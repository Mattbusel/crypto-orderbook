use std::sync::{Arc, RwLock};
use std::time::Duration;

use rust_decimal::Decimal;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::error::{AppError, Result};
use crate::orderbook::book::OrderBook;
use crate::ws::binance::{parse_depth_update, DepthUpdate, PriceLevel};
use crate::ws::WsEvent;

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
    // Where we receive WebSocket events from the client task.
    rx: mpsc::Receiver<WsEvent>,

    // The order book shared with the API layer.
    // std::sync::RwLock because book operations are pure computation —
    // no await points inside the lock, so we don't need the async version.
    book: Arc<RwLock<OrderBook>>,

    // Trading pair, e.g. "BTCUSDT". Used to build the REST snapshot URL.
    symbol: String,

    // HTTP client. reqwest clients hold a connection pool internally,
    // so we create one and reuse it rather than making a new one per request.
    http: reqwest::Client,
}

impl Manager {
    pub fn new(
        rx: mpsc::Receiver<WsEvent>,
        book: Arc<RwLock<OrderBook>>,
        symbol: String,
    ) -> Self {
        Self {
            rx,
            book,
            symbol,
            http: reqwest::Client::new(),
        }
    }

    /// Run the manager indefinitely.
    ///
    /// The outer loop is the resync loop. Every time we need to start over —
    /// because of a reconnect, a sequence gap, or a failed handshake —
    /// we return to the top and begin the snapshot fetch again.
    pub async fn run(mut self) {
        loop {
            info!(symbol = %self.symbol, "starting sync cycle");

            // Phase 1: Fetch a fresh snapshot from Binance's REST API.
            // While this is happening, the WebSocket client is still running
            // and events are queuing up in the mpsc channel's internal buffer.
            let snapshot = self.fetch_snapshot_with_retry().await;

            // Apply the snapshot to the book. This gives us a known baseline
            // to start applying incremental updates on top of.
            {
                let mut book = self.book.write().expect("book lock poisoned");
                book.clear();
                apply_snapshot_to_book(&mut book, &snapshot);
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
    /// We never give up — if Binance's REST is down, we wait.
    async fn fetch_snapshot_with_retry(&self) -> Snapshot {
        let mut attempt = 0u32;
        loop {
            match self.fetch_snapshot().await {
                Ok(snap) => return snap,
                Err(e) => {
                    let wait_secs = 2u64.saturating_pow(attempt).min(30);
                    error!(error = %e, wait_secs, "snapshot fetch failed, retrying");
                    tokio::time::sleep(Duration::from_secs(wait_secs)).await;
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    }

    async fn fetch_snapshot(&self) -> Result<Snapshot> {
        let url = format!(
            "https://api.binance.com/api/v3/depth?symbol={}&limit=5000",
            self.symbol
        );

        let raw: RawSnapshot = self.http
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::InvalidField(format!("snapshot HTTP error: {e}")))?
            .json()
            .await
            .map_err(|_| AppError::InvalidField("snapshot JSON parse failed".into()))?;

        let bids = parse_snapshot_levels(raw.bids)?;
        let asks = parse_snapshot_levels(raw.asks)?;

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
            let event = self.rx.recv().await?; // None = channel closed = shut down

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

                    // Stale: this event's last update is before or at our snapshot.
                    // All the changes it describes are already in the snapshot.
                    if update.last_update_id <= snapshot_last_id {
                        continue;
                    }

                    // Gap: the first new event starts after the snapshot's sequence.
                    // We missed at least one update between the snapshot and this event.
                    // The book is unrecoverable — signal a resync.
                    if update.first_update_id > snapshot_last_id + 1 {
                        error!(
                            snapshot_last_id,
                            event_first = update.first_update_id,
                            "sync gap: first event does not connect to snapshot"
                        );
                        return None;
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
            let event = match self.rx.recv().await {
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

                    // The sequence continuity check.
                    // Each event's first_update_id must immediately follow
                    // the previous event's last_update_id.
                    // A gap here means we missed at least one update.
                    if update.first_update_id != prev_last_id + 1 {
                        return LiveResult::SequenceGap {
                            expected: prev_last_id + 1,
                            got: update.first_update_id,
                        };
                    }

                    self.book
                        .write()
                        .expect("book lock poisoned")
                        .apply(&update);

                    prev_last_id = update.last_update_id;
                }
            }
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
