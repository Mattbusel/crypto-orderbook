use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::api::ws_push::{serialize_push, PushSender};
use crate::error::{AppError, Result};
use crate::metrics;
use crate::orderbook::book::OrderBook;
use crate::orderbook::sync::{self, Handshake, Live, Market};
use crate::ws::binance::{parse_depth_update, DepthUpdate, PriceLevel};
use crate::ws::WsEvent;

// ── Circuit breaker ────────────────────────────────────────────────────────────
// Wraps the snapshot REST call. Opens after OPEN_THRESHOLD consecutive failures,
// rejects requests for COOLDOWN, then lets one probe through (HalfOpen).
// This prevents hammering a degraded Binance endpoint, which would consume
// our rate-limit quota and make recovery slower, not faster.

const OPEN_THRESHOLD: u32     = 5;
const COOLDOWN: Duration       = Duration::from_secs(30);

#[derive(Debug)]
enum BreakerState {
    Closed { consecutive_failures: u32 },
    Open   { opened_at: Instant },
    HalfOpen,
}

struct CircuitBreaker {
    state: BreakerState,
}

impl CircuitBreaker {
    fn new() -> Self {
        Self { state: BreakerState::Closed { consecutive_failures: 0 } }
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
        match self.state {
            BreakerState::HalfOpen => {
                info!("circuit breaker closed: probe succeeded");
            }
            _ => {}
        }
        self.state = BreakerState::Closed { consecutive_failures: 0 };
    }

    fn record_failure(&mut self) {
        self.state = match self.state {
            BreakerState::Closed { consecutive_failures } => {
                let n = consecutive_failures + 1;
                if n >= OPEN_THRESHOLD {
                    warn!(failures = n, "circuit breaker OPEN: blocking snapshot requests");
                    BreakerState::Open { opened_at: Instant::now() }
                } else {
                    BreakerState::Closed { consecutive_failures: n }
                }
            }
            BreakerState::HalfOpen => {
                warn!("circuit breaker re-opened: probe failed");
                BreakerState::Open { opened_at: Instant::now() }
            }
            BreakerState::Open { opened_at } => BreakerState::Open { opened_at },
        };
    }
}

// ── Snapshot types ─────────────────────────────────────────────────────────────

// The shape Binance's REST depth endpoint returns.
// Private to this module — nothing else needs to know about the REST format.
#[derive(Deserialize)]
struct RawSnapshot {
    #[serde(rename = "lastUpdateId")]
    last_update_id: u64,
    bids: Vec<[String; 2]>,
    asks: Vec<[String; 2]>,
}

// Our internal representation after parsing strings into Decimals.
struct Snapshot {
    last_update_id: u64,
    bids: Vec<PriceLevel>,
    asks: Vec<PriceLevel>,
}

// ── Manager ────────────────────────────────────────────────────────────────────

pub struct Manager {
    rx:       mpsc::Receiver<WsEvent>,
    book:     Arc<RwLock<OrderBook>>,
    symbol:   String,
    http:     reqwest::Client,
    push_tx:  PushSender,
    breaker:  CircuitBreaker,
    market:   Market,
    rest_base: String,
    depth_limit: u32,
    // Events taken off the channel before the snapshot (the first depth
    // event of a cycle), replayed to the handshake in order.
    pending:  std::collections::VecDeque<WsEvent>,
    // RECORD_FILE: every snapshot and raw depth event, for replay.
    recorder: Option<crate::orderbook::replay::Recorder>,
}

impl Manager {
    pub fn new(
        rx:      mpsc::Receiver<WsEvent>,
        book:    Arc<RwLock<OrderBook>>,
        symbol:  String,
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
            pending: std::collections::VecDeque::new(),
            recorder: None,
        }
    }

    /// Record every snapshot and raw depth event to `recorder` (see
    /// [`crate::orderbook::replay`]).
    pub fn with_recorder(mut self, recorder: crate::orderbook::replay::Recorder) -> Self {
        self.recorder = Some(recorder);
        self
    }

    /// The next event: anything held back first, then the channel.
    async fn next_event(&mut self) -> Option<WsEvent> {
        if let Some(event) = self.pending.pop_front() {
            return Some(event);
        }
        let event = self.rx.recv().await?;
        if let (Some(rec), WsEvent::Message(raw)) = (self.recorder.as_mut(), &event) {
            if let Err(e) = rec.depth(raw) {
                warn!(error = %e, "could not write to RECORD_FILE; recording stopped");
                self.recorder = None;
            }
        }
        Some(event)
    }

    /// Wait until the depth stream delivers an event, hold it for the
    /// handshake, and return its first update id (`U`). Binance's procedure
    /// starts here: a snapshot fetched before the stream is flowing can be
    /// older than the stream's first event, which forces a second snapshot.
    /// Returns `None` if the channel closed.
    async fn wait_for_stream(&mut self) -> Option<u64> {
        loop {
            let event = self.next_event().await?;
            if let WsEvent::Message(raw) = &event {
                if let Ok(update) = parse_depth_update(raw) {
                    let first = update.first_update_id;
                    self.pending.push_front(event);
                    return Some(first);
                }
            }
            // Reconnect signals and unparsable frames before the first event
            // carry nothing to keep.
        }
    }

    /// Follow `market` (spot or USD-M futures), fetching snapshots of
    /// `depth_limit` levels from `rest_base`.
    pub fn with_market(mut self, market: Market, rest_base: String, depth_limit: u32) -> Self {
        self.market = market;
        self.rest_base = rest_base;
        self.depth_limit = depth_limit.clamp(5, market.max_depth_limit());
        self
    }

    /// Run the manager indefinitely.
    ///
    /// The outer loop is the resync loop. Every time we need to start over —
    /// because of a reconnect, a sequence gap, or a failed handshake —
    /// we return to the top and begin the snapshot fetch again.
    pub async fn run(mut self) {
        loop {
            info!(symbol = %self.symbol, "starting sync cycle");

            // Phase 0: wait for the stream to deliver its first event, so the
            // snapshot is never older than the stream (Binance's procedure).
            let Some(first_u) = self.wait_for_stream().await else {
                info!("event channel closed, shutting down manager");
                return;
            };

            // Phase 1: Fetch a fresh snapshot from Binance's REST API.
            // While this is happening, the WebSocket client is still running
            // and events are queuing up in the mpsc channel's internal buffer.
            // If the snapshot is older than the first buffered event, fetch
            // again rather than restart the whole cycle.
            let mut snapshot = self.fetch_snapshot_with_retry().await;
            let mut refetches = 0;
            while snapshot.last_update_id < first_u && refetches < 5 {
                refetches += 1;
                tokio::time::sleep(Duration::from_millis(250)).await;
                snapshot = self.fetch_snapshot_with_retry().await;
            }

            // Apply the snapshot to the book. This gives us a known baseline
            // to start applying incremental updates on top of.
            {
                let mut book = self.book.write().expect("book lock poisoned");
                book.clear();
                apply_snapshot_to_book(&mut book, &snapshot);
                if let Some(rec) = self.recorder.as_mut() {
                    if let Err(e) = rec.snapshot(self.market, snapshot.last_update_id, &snapshot.bids, &snapshot.asks) {
                        warn!(error = %e, "could not write to RECORD_FILE; recording stopped");
                        self.recorder = None;
                    }
                }
                info!(last_update_id = snapshot.last_update_id, "snapshot applied");
            }

            // Phase 2: Find the first event that connects to the snapshot.
            // Events that arrived before or overlapping the snapshot get discarded.
            let first_last_id = match self.find_first_valid_event(snapshot.last_update_id).await {
                Some(id) => id,
                None => {
                    // Reconnected during handshake — start the whole cycle over.
                    self.book.write().expect("book lock poisoned").clear();
                    continue;
                }
            };

            info!(first_last_id, "sync handshake complete, entering live mode");

            // Phase 3: Live mode. Apply every event, validating sequence continuity.
            match self.run_live(first_last_id).await {
                LiveResult::SequenceGap { expected, got } => {
                    metrics::SEQUENCE_GAPS_TOTAL.inc();
                    error!(expected, got, "sequence gap detected, resyncing");
                }
                LiveResult::Reconnected => {
                    info!("reconnect signal received, resyncing");
                }
                LiveResult::ChannelClosed => {
                    info!("event channel closed, shutting down manager");
                    return;
                }
            }

            // Any exit from live mode means the book is now unreliable.
            // Clear it so the API doesn't serve stale data while we resync.
            self.book.write().expect("book lock poisoned").clear();
        }
    }

    // ── Phase 1: Snapshot fetch ──────────────────────────────────────────────

    /// Fetch a depth snapshot, retrying with backoff until one arrives.
    /// The circuit breaker stops outbound requests during Binance REST outages.
    async fn fetch_snapshot_with_retry(&mut self) -> Snapshot {
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
                Err(SnapshotError::RateLimited { status, retry_after_secs }) => {
                    // Binance: keep sending after a 429 and the IP is banned
                    // (418) for minutes to days. Wait exactly as long as asked.
                    metrics::SNAPSHOT_RATE_LIMITED_TOTAL.inc();
                    let wait = retry_after_secs.unwrap_or(if status == 418 { 120 } else { 60 });
                    error!(status, wait_secs = wait, "Binance rate limit on the snapshot; waiting as instructed");
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                }
                Err(SnapshotError::Other(e)) => {
                    self.breaker.record_failure();
                    let wait_secs = 2u64.saturating_pow(attempt).min(30);
                    error!(error = %e, wait_secs, "snapshot fetch failed, retrying");
                    tokio::time::sleep(Duration::from_secs(wait_secs)).await;
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    }

    async fn fetch_snapshot(&self) -> std::result::Result<Snapshot, SnapshotError> {
        // REST_BASE lets the snapshot come from another Binance deployment
        // (e.g. https://api.binance.us where binance.com is geo-blocked).
        let url = format!(
            "{}{}?symbol={}&limit={}",
            self.rest_base.trim_end_matches('/'),
            self.market.depth_path(),
            self.symbol,
            self.depth_limit
        );

        let resp = self.http
            .get(&url)
            .send()
            .await
            .map_err(|e| SnapshotError::Other(AppError::InvalidField(format!("snapshot HTTP error: {e}"))))?;
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
            return Err(SnapshotError::RateLimited { status, retry_after_secs });
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SnapshotError::Other(AppError::InvalidField(format!(
                "snapshot HTTP {status}: {}",
                body.chars().take(200).collect::<String>()
            ))));
        }
        let raw: RawSnapshot = resp
            .json()
            .await
            .map_err(|e| SnapshotError::Other(AppError::InvalidField(format!("snapshot JSON parse failed: {e}"))))?;

        let bids = parse_snapshot_levels(raw.bids).map_err(SnapshotError::Other)?;
        let asks = parse_snapshot_levels(raw.asks).map_err(SnapshotError::Other)?;

        Ok(Snapshot {
            last_update_id: raw.last_update_id,
            bids,
            asks,
        })
    }

    // ── Phase 2: Handshake ───────────────────────────────────────────────────

    /// Scan incoming events until we find the first one that cleanly connects
    /// to the snapshot we just applied.
    ///
    /// Returns the last_update_id of the first valid event (used as the
    /// starting point for sequence continuity checking in live mode),
    /// or None if we received a Reconnected signal and need to start over.
    async fn find_first_valid_event(&mut self, snapshot_last_id: u64) -> Option<u64> {
        loop {
            let event = self.next_event().await?; // None = channel closed = shut down

            match event {
                WsEvent::Reconnected => {
                    // The WebSocket dropped while we were doing the handshake.
                    // The events buffered so far are now from a dead stream.
                    return None;
                }
                WsEvent::Message(raw) => {
                    let update = match parse_depth_update(&raw) {
                        Ok(u) => u,
                        Err(e) => {
                            warn!(error = %e, "parse error during handshake, skipping");
                            continue;
                        }
                    };

                    match sync::handshake(self.market, snapshot_last_id, &update) {
                        // Already contained in the snapshot.
                        Handshake::Stale => continue,
                        // Updates between the snapshot and this event were
                        // missed: fetch a new snapshot.
                        Handshake::Gap => {
                            error!(
                                snapshot_last_id,
                                event_first = update.first_update_id,
                                "sync gap: first event does not connect to snapshot"
                            );
                            return None;
                        }
                        Handshake::Apply => {}
                    }

                    // This event either overlaps or directly follows the snapshot.
                    // Binance's spec says this is the correct first event to apply.
                    self.book
                        .write()
                        .expect("book lock poisoned")
                        .apply(&update);

                    return Some(update.last_update_id);
                }
            }
        }
    }

    // ── Phase 3: Live mode ───────────────────────────────────────────────────

    /// Process events indefinitely, checking that each one's sequence number
    /// directly follows the previous one. Returns when something breaks.
    async fn run_live(&mut self, first_last_id: u64) -> LiveResult {
        let mut prev_last_id = first_last_id;

        loop {
            let event = match self.next_event().await {
                Some(e) => e,
                None => return LiveResult::ChannelClosed,
            };

            match event {
                WsEvent::Reconnected => return LiveResult::Reconnected,

                WsEvent::Message(raw) => {
                    let update = match parse_depth_update(&raw) {
                        Ok(u) => u,
                        Err(e) => {
                            warn!(error = %e, "parse error in live mode, skipping frame");
                            continue;
                        }
                    };

                    // Sequence continuity (spot: U follows the previous u;
                    // futures: pu equals the previous u). A duplicate or late
                    // event is skipped instead of forcing a resync.
                    match sync::live(self.market, prev_last_id, &update) {
                        Live::Apply => {}
                        Live::Ignore => continue,
                        Live::Gap { expected, got } => {
                            return LiveResult::SequenceGap { expected, got };
                        }
                    }

                    {
                        let mut book = self.book.write().expect("book lock poisoned");
                        book.apply(&update);
                        metrics::UPDATES_APPLIED_TOTAL.inc();
                        metrics::WS_EVENTS_TOTAL.inc();
                        metrics::BID_DEPTH.set(book.bid_depth() as f64);
                        metrics::ASK_DEPTH.set(book.ask_depth() as f64);
                        if let Some(s) = book.spread() {
                            use rust_decimal::prelude::ToPrimitive;
                            if let Some(f) = s.to_f64() {
                                metrics::SPREAD.set(f);
                            }
                        }
                        // Publish to WebSocket subscribers. send() fails only when
                        // there are no subscribers — that's fine, we discard the error.
                        if let Some(payload) = serialize_push(
                            &self.symbol,
                            book.best_bid(),
                            book.best_ask(),
                            book.spread(),
                            book.micro_price(),
                            book.last_update_id,
                        ) {
                            let _ = self.push_tx.send(payload);
                        }
                    }

                    prev_last_id = update.last_update_id;
                }
            }
        }
    }
}

// Why a snapshot request failed: a rate limit is waited out as instructed,
// anything else is retried with backoff.
enum SnapshotError {
    RateLimited { status: u16, retry_after_secs: Option<u64> },
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

// ── Result type for live mode ──────────────────────────────────────────────────

// Named enum so the caller knows exactly why live mode ended.
// Each variant maps to a different log message and the same recovery action
// (start the sync cycle over), but naming them makes the code self-documenting.
enum LiveResult {
    SequenceGap { expected: u64, got: u64 },
    Reconnected,
    ChannelClosed,
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn apply_snapshot_to_book(book: &mut OrderBook, snapshot: &Snapshot) {
    // We apply the snapshot as if it were a single update that sets all levels.
    // The book was just cleared, so there are no stale entries to worry about.
    let update = DepthUpdate {
        first_update_id: snapshot.last_update_id,
        last_update_id: snapshot.last_update_id,
        prev_last_update_id: None,
        bids: snapshot.bids.clone(),
        asks: snapshot.asks.clone(),
    };
    book.apply(&update);
}

fn parse_snapshot_levels(raw: Vec<[String; 2]>) -> Result<Vec<PriceLevel>> {
    raw.into_iter()
        .map(|entry| {
            let price = entry[0]
                .parse::<Decimal>()
                .map_err(|e| AppError::InvalidField(format!("bad snapshot price '{}': {e}", entry[0])))?;
            let quantity = entry[1]
                .parse::<Decimal>()
                .map_err(|e| AppError::InvalidField(format!("bad snapshot qty '{}': {e}", entry[1])))?;
            Ok(PriceLevel { price, quantity })
        })
        .collect()
}
