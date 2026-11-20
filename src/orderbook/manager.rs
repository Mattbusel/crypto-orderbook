//! The networked driver: reads WebSocket frames from a channel, fetches REST
//! snapshots when [`BookSync`] asks for one, and publishes the book to the
//! HTTP API, WebSocket subscribers and Prometheus. All sequencing decisions
//! are made by [`BookSync`]; this module only does I/O around it.

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::api::ws_push::{serialize_push, PushSender};
use crate::error::AppError;
use crate::metrics;
use crate::orderbook::book::OrderBook;
use crate::orderbook::sync::{BookSync, Market, Outcome, ResyncReason, SyncState};
use crate::ws::binance::{parse_depth_snapshot, parse_depth_update, DepthSnapshot, DepthUpdate};
use crate::ws::WsEvent;

// ── Circuit breaker ────────────────────────────────────────────────────────────
// Wraps the snapshot REST call. Opens after OPEN_THRESHOLD consecutive failures,
// rejects requests for COOLDOWN, then lets one probe through (HalfOpen).
// This prevents hammering a degraded Binance endpoint, which would consume
// our rate-limit quota and make recovery slower, not faster.

const OPEN_THRESHOLD: u32 = 5;
const COOLDOWN: Duration = Duration::from_secs(30);

// How many times to refetch a snapshot that is older than the first buffered
// event before giving up on this cycle and letting BookSync resync.
const MAX_REFETCHES: u32 = 5;

#[derive(Debug)]
enum BreakerState {
    Closed { consecutive_failures: u32 },
    Open { opened_at: Instant },
    HalfOpen,
}

struct CircuitBreaker {
    state: BreakerState,
}

impl CircuitBreaker {
    fn new() -> Self {
        Self {
            state: BreakerState::Closed {
                consecutive_failures: 0,
            },
        }
    }

    /// Returns true if the caller should attempt the request.
    fn allow(&mut self) -> bool {
        match &self.state {
            BreakerState::Closed { .. } => true,
            BreakerState::Open { opened_at } => {
                if opened_at.elapsed() >= COOLDOWN {
                    self.state = BreakerState::HalfOpen;
                    info!("circuit breaker half-open: allowing probe request");
                    true
                } else {
                    false
                }
            }
            BreakerState::HalfOpen => true,
        }
    }

    fn record_success(&mut self) {
        if let BreakerState::HalfOpen = self.state {
            info!("circuit breaker closed: probe succeeded");
        }
        self.state = BreakerState::Closed {
            consecutive_failures: 0,
        };
    }

    fn record_failure(&mut self) {
        self.state = match self.state {
            BreakerState::Closed {
                consecutive_failures,
            } => {
                let n = consecutive_failures + 1;
                if n >= OPEN_THRESHOLD {
                    warn!(
                        failures = n,
                        "circuit breaker OPEN: blocking snapshot requests"
                    );
                    BreakerState::Open {
                        opened_at: Instant::now(),
                    }
                } else {
                    BreakerState::Closed {
                        consecutive_failures: n,
                    }
                }
            }
            BreakerState::HalfOpen => {
                warn!("circuit breaker re-opened: probe failed");
                BreakerState::Open {
                    opened_at: Instant::now(),
                }
            }
            BreakerState::Open { opened_at } => BreakerState::Open { opened_at },
        };
    }
}

// ── Manager ────────────────────────────────────────────────────────────────────

/// Keeps a shared [`OrderBook`] in sync with Binance.
///
/// Feed it the depth stream through `rx` (see [`crate::ws::client::spawn`])
/// and call [`run`](Self::run). Readers lock `book` to query it.
pub struct Manager {
    rx: mpsc::Receiver<WsEvent>,
    book: Arc<RwLock<OrderBook>>,
    symbol: String,
    http: reqwest::Client,
    push_tx: PushSender,
    breaker: CircuitBreaker,
    market: Market,
    rest_base: String,
    depth_limit: u32,
    max_buffer: usize,
    // RECORD_FILE: every snapshot and raw depth event, for replay.
    recorder: Option<crate::orderbook::replay::Recorder>,
}

/// What the stream delivered next.
enum Next {
    Update(DepthUpdate),
    Reconnected,
}

impl Manager {
    /// A manager for `symbol` on spot with the default REST base; adjust
    /// with [`with_market`](Self::with_market).
    pub fn new(
        rx: mpsc::Receiver<WsEvent>,
        book: Arc<RwLock<OrderBook>>,
        symbol: String,
        push_tx: PushSender,
    ) -> Self {
        Self {
            rx,
            book,
            symbol,
            push_tx,
            http: reqwest::Client::new(),
            breaker: CircuitBreaker::new(),
            market: Market::Spot,
            rest_base: Market::Spot.default_rest_base().to_string(),
            depth_limit: Market::Spot.max_depth_limit(),
            max_buffer: BookSync::DEFAULT_MAX_BUFFER,
            recorder: None,
        }
    }

    /// Record every snapshot and raw depth event to `recorder` (see
    /// [`crate::orderbook::replay`]).
    pub fn with_recorder(mut self, recorder: crate::orderbook::replay::Recorder) -> Self {
        self.recorder = Some(recorder);
        self
    }

    /// Follow `market` (spot or USD-M futures), fetching snapshots of
    /// `depth_limit` levels from `rest_base`.
    pub fn with_market(mut self, market: Market, rest_base: String, depth_limit: u32) -> Self {
        self.market = market;
        self.rest_base = rest_base;
        self.depth_limit = depth_limit.clamp(5, market.max_depth_limit());
        self
    }

    /// Hold at most `max` events while waiting for a snapshot.
    pub fn with_max_buffer(mut self, max: usize) -> Self {
        self.max_buffer = max.max(1);
        self
    }

    /// The next parsed depth event or reconnect signal; `None` once the
    /// channel is closed. Every raw frame is recorded as it arrives.
    async fn next(&mut self) -> Option<Next> {
        loop {
            let event = self.rx.recv().await?;
            match event {
                WsEvent::Reconnected => {
                    metrics::WS_RECONNECTS_TOTAL.inc();
                    return Some(Next::Reconnected);
                }
                WsEvent::Message(raw) => {
                    if let Some(rec) = self.recorder.as_mut() {
                        if let Err(e) = rec.depth(&raw) {
                            warn!(error = %e, "could not write to RECORD_FILE; recording stopped");
                            self.recorder = None;
                        }
                    }
                    match parse_depth_update(&raw) {
                        Ok(update) => {
                            metrics::WS_EVENTS_TOTAL.inc();
                            return Some(Next::Update(update));
                        }
                        Err(e) => {
                            metrics::PARSE_ERRORS_TOTAL.inc();
                            warn!(error = %e, "unparsable depth frame, skipping");
                        }
                    }
                }
            }
        }
    }

    /// Run the manager until the event channel closes.
    ///
    /// Binance's procedure, driven by [`BookSync`]: buffer the stream, fetch
    /// a snapshot once it has delivered an event (refetching while the
    /// snapshot is older than that event), replay the buffer onto it, then
    /// follow the stream. A gap or reconnect clears the book and starts over.
    pub async fn run(mut self) {
        let mut sync = BookSync::new(self.market).with_max_buffer(self.max_buffer);
        loop {
            // Phase 0: wait until the stream has delivered an event, so the
            // snapshot is never older than the stream.
            while !sync.needs_snapshot() {
                match self.next().await {
                    None => {
                        info!("event channel closed, shutting down manager");
                        return;
                    }
                    Some(Next::Reconnected) => self.reset(&mut sync),
                    Some(Next::Update(update)) => {
                        let mut book = self.book.write().expect("book lock poisoned");
                        sync.on_event(&mut book, update);
                    }
                }
            }
            info!(symbol = %self.symbol, first_update_id = sync.first_buffered_update_id(), "starting sync cycle");

            // Phase 1: snapshot. The WebSocket keeps running meanwhile and its
            // events queue in the channel; BookSync sees them after the snapshot.
            let mut snapshot = self.fetch_snapshot_with_retry().await;
            let mut refetches = 0;
            while sync.snapshot_is_too_old(snapshot.last_update_id) && refetches < MAX_REFETCHES {
                refetches += 1;
                metrics::SNAPSHOT_REFETCHES_TOTAL.inc();
                warn!(
                    snapshot_id = snapshot.last_update_id,
                    first_update_id = sync.first_buffered_update_id(),
                    "snapshot older than the stream, fetching again"
                );
                tokio::time::sleep(Duration::from_millis(250)).await;
                snapshot = self.fetch_snapshot_with_retry().await;
            }
            if let Some(rec) = self.recorder.as_mut() {
                if let Err(e) = rec.snapshot(
                    self.market,
                    snapshot.last_update_id,
                    &snapshot.bids,
                    &snapshot.asks,
                ) {
                    warn!(error = %e, "could not write to RECORD_FILE; recording stopped");
                    self.recorder = None;
                }
            }

            // Phase 2: replay the buffer onto the snapshot.
            let snapshot_id = snapshot.last_update_id;
            let done = {
                let mut book = self.book.write().expect("book lock poisoned");
                sync.on_snapshot(&mut book, snapshot)
            };
            info!(
                snapshot_id,
                applied = done.applied,
                skipped = done.skipped,
                "snapshot applied"
            );
            metrics::EVENTS_SKIPPED_TOTAL.inc_by(done.skipped as f64);
            metrics::UPDATES_APPLIED_TOTAL.inc_by(done.applied as f64);
            if let Some(reason) = done.resync {
                self.on_resync(reason);
                self.publish_state(&sync);
                continue;
            }
            self.publish_state(&sync);
            if sync.is_live() {
                info!("sync handshake complete, entering live mode");
            }

            // Phase 3: follow the stream until something breaks.
            loop {
                let update = match self.next().await {
                    None => {
                        info!("event channel closed, shutting down manager");
                        return;
                    }
                    Some(Next::Reconnected) => {
                        info!("reconnect signal received, resyncing");
                        self.reset(&mut sync);
                        break;
                    }
                    Some(Next::Update(update)) => update,
                };
                let was_live = sync.is_live();
                let mut book = self.book.write().expect("book lock poisoned");
                match sync.on_event(&mut book, update) {
                    Outcome::Applied => {
                        metrics::UPDATES_APPLIED_TOTAL.inc();
                        publish_book(&book);
                        if let Some(payload) = serialize_push(
                            &self.symbol,
                            book.best_bid(),
                            book.best_ask(),
                            book.spread(),
                            book.micro_price(),
                            book.last_update_id,
                        ) {
                            // send() fails only when nobody is subscribed.
                            let _ = self.push_tx.send(payload);
                        }
                        drop(book);
                        if !was_live {
                            info!("sync handshake complete, entering live mode");
                            self.publish_state(&sync);
                        }
                    }
                    Outcome::Skipped => metrics::EVENTS_SKIPPED_TOTAL.inc(),
                    Outcome::Buffered => {}
                    Outcome::Resync(reason) => {
                        drop(book);
                        self.on_resync(reason);
                        self.publish_state(&sync);
                        break;
                    }
                }
            }
        }
    }

    fn reset(&self, sync: &mut BookSync) {
        let mut book = self.book.write().expect("book lock poisoned");
        sync.reset(&mut book);
        drop(book);
        self.publish_state(sync);
    }

    fn on_resync(&self, reason: ResyncReason) {
        metrics::RESYNCS_TOTAL.inc();
        match reason {
            ResyncReason::Gap { expected, got } => {
                metrics::SEQUENCE_GAPS_TOTAL.inc();
                error!(expected, got, "sequence gap detected, resyncing");
            }
            ResyncReason::SnapshotTooOld {
                snapshot_id,
                first_update_id,
            } => {
                error!(
                    snapshot_id,
                    first_update_id, "snapshot does not connect to the stream, resyncing"
                );
            }
        }
    }

    fn publish_state(&self, sync: &BookSync) {
        let (synced, last) = match sync.state() {
            SyncState::Live { last_update_id } => (1.0, last_update_id as f64),
            _ => (0.0, 0.0),
        };
        metrics::SYNCED.set(synced);
        metrics::LAST_UPDATE_ID.set(last);
        metrics::BUFFERED_EVENTS.set(sync.buffered() as f64);
        if !sync.is_live() {
            metrics::BID_DEPTH.set(0.0);
            metrics::ASK_DEPTH.set(0.0);
        }
    }

    // ── Snapshot fetch ───────────────────────────────────────────────────────

    /// Fetch a depth snapshot, retrying with backoff until one arrives.
    /// The circuit breaker stops outbound requests during Binance REST outages.
    async fn fetch_snapshot_with_retry(&mut self) -> DepthSnapshot {
        let mut attempt = 0u32;
        loop {
            if !self.breaker.allow() {
                // Circuit is open. Don't hammer Binance. Wait then retry.
                warn!("circuit breaker open, waiting 5s before retry");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            match self.fetch_snapshot().await {
                Ok(snap) => {
                    self.breaker.record_success();
                    metrics::SNAPSHOTS_TOTAL.inc();
                    return snap;
                }
                Err(SnapshotError::RateLimited {
                    status,
                    retry_after_secs,
                }) => {
                    // Binance: keep sending after a 429 and the IP is banned
                    // (418) for minutes to days. Wait exactly as long as asked.
                    metrics::SNAPSHOT_RATE_LIMITED_TOTAL.inc();
                    let wait = retry_after_secs.unwrap_or(if status == 418 { 120 } else { 60 });
                    error!(
                        status,
                        wait_secs = wait,
                        "Binance rate limit on the snapshot; waiting as instructed"
                    );
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                }
                Err(SnapshotError::Other(e)) => {
                    self.breaker.record_failure();
                    metrics::SNAPSHOT_FAILURES_TOTAL.inc();
                    let wait_secs = 2u64.saturating_pow(attempt).min(30);
                    error!(error = %e, wait_secs, "snapshot fetch failed, retrying");
                    tokio::time::sleep(Duration::from_secs(wait_secs)).await;
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    }

    async fn fetch_snapshot(&self) -> std::result::Result<DepthSnapshot, SnapshotError> {
        // REST_BASE lets the snapshot come from another Binance deployment
        // (e.g. https://api.binance.us where binance.com is geo-blocked).
        let url = format!(
            "{}{}?symbol={}&limit={}",
            self.rest_base.trim_end_matches('/'),
            self.market.depth_path(),
            self.symbol,
            self.depth_limit
        );

        let resp = self.http.get(&url).send().await.map_err(|e| {
            SnapshotError::Other(AppError::InvalidField(format!("snapshot HTTP error: {e}")))
        })?;
        if let Some(weight) = resp
            .headers()
            .get("x-mbx-used-weight-1m")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok())
        {
            metrics::REST_USED_WEIGHT_1M.set(weight);
        }
        let status = resp.status().as_u16();
        if status == 429 || status == 418 {
            let retry_after_secs = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            return Err(SnapshotError::RateLimited {
                status,
                retry_after_secs,
            });
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SnapshotError::Other(AppError::InvalidField(format!(
                "snapshot HTTP {status}: {}",
                body.chars().take(200).collect::<String>()
            ))));
        }
        let body = resp.text().await.map_err(|e| {
            SnapshotError::Other(AppError::InvalidField(format!(
                "snapshot body read failed: {e}"
            )))
        })?;
        parse_depth_snapshot(&body).map_err(SnapshotError::Other)
    }
}

/// Refresh the book gauges after an update.
fn publish_book(book: &OrderBook) {
    metrics::BID_DEPTH.set(book.bid_depth() as f64);
    metrics::ASK_DEPTH.set(book.ask_depth() as f64);
    metrics::LAST_UPDATE_ID.set(book.last_update_id as f64);
    if let Some(s) = book.spread() {
        use rust_decimal::prelude::ToPrimitive;
        if let Some(f) = s.to_f64() {
            metrics::SPREAD.set(f);
        }
    }
}

// Why a snapshot request failed: a rate limit is waited out as instructed,
// anything else is retried with backoff.
enum SnapshotError {
    RateLimited {
        status: u16,
        retry_after_secs: Option<u64>,
    },
    Other(AppError),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimited { status, .. } => write!(f, "rate limited (HTTP {status})"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}
