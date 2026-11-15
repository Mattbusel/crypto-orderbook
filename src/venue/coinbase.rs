//! Coinbase Advanced Trade `level2` order book as a sans-IO state machine.
//!
//! The rules ([level2 channel](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/websocket/level2)):
//!
//! - Subscribe with `{"type":"subscribe","channel":"level2","product_ids":[..]}`
//!   on `wss://advanced-trade-ws.coinbase.com`. Messages come back on
//!   channel `l2_data`. The first event for a product is a `snapshot` of the
//!   whole book; later events are `update`s.
//! - Each update carries `side` (`bid` or `offer`), `price_level` and
//!   `new_quantity`: the new total at that price, not a delta. `"0"` removes
//!   the level.
//! - Every message on the connection carries `sequence_num`, a
//!   per-connection counter that grows by exactly one per message, across
//!   all channels on that connection (heartbeats included). A jump means a
//!   message was lost; a smaller or repeated number is a late or duplicate
//!   message and is ignored.
//!
//! So [`CoinbaseSync`] has to see **every** frame of the connection, not
//! only the `l2_data` ones: feed heartbeats and subscription acks too, and
//! they advance the counter. A jump while the book is live drops the book
//! ([`GapReason::Sequence`]); unsubscribe and subscribe again (Coinbase then
//! sends a fresh snapshot) or reconnect and call [`CoinbaseSync::reset`].

// The crate-wide `AppError` is large (it can hold a WebSocket error); the
// parsers return it like the Binance ones do.
#![allow(clippy::result_large_err)]

use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{merge, FeedOutcome, GapReason, StreamState, Venue, VenueSync};
use crate::error::{AppError, Result};
use crate::orderbook::book::{OrderBook, Side};
use crate::orderbook::sync::SyncStats;

/// Coinbase Advanced Trade market data WebSocket.
pub const WS_URL: &str = "wss://advanced-trade-ws.coinbase.com";

// ── Wire format ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RawEnvelope {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    sequence_num: Option<u64>,
    #[serde(default)]
    events: Option<Box<RawValue>>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct RawL2Event {
    #[serde(rename = "type")]
    kind: String,
    product_id: String,
    #[serde(default)]
    updates: Vec<RawL2Update>,
}

#[derive(Deserialize)]
struct RawL2Update {
    side: String,
    price_level: String,
    new_quantity: String,
}

// ── Domain types ─────────────────────────────────────────────────────────────

/// One frame from the Advanced Trade WebSocket, reduced to what book sync
/// needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoinbaseMessage {
    /// The envelope's `channel` (`l2_data`, `heartbeats`, `subscriptions`, ...).
    pub channel: Option<String>,
    /// The per-connection `sequence_num`, if the frame carries one.
    pub sequence_num: Option<u64>,
    /// The `level2` events; empty for every other channel.
    pub events: Vec<CoinbaseEvent>,
    /// The `message` of an `{"type":"error"}` frame.
    pub error: Option<String>,
}

/// Whether a `level2` event replaces the book or changes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoinbaseEventKind {
    /// The whole book for the product.
    Snapshot,
    /// Changes to some levels.
    Update,
}

/// One `level2` event: a snapshot or update for one product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoinbaseEvent {
    /// Snapshot or update.
    pub kind: CoinbaseEventKind,
    /// The product, for example `BTC-USD`.
    pub product_id: String,
    /// `(side, price, new total quantity)`, in the order Coinbase sent them.
    pub updates: Vec<(Side, Decimal, Decimal)>,
}

fn decimal(field: &str, s: &str) -> Result<Decimal> {
    s.parse::<Decimal>()
        .or_else(|_| Decimal::from_scientific(s))
        .map_err(|e| AppError::InvalidField(format!("invalid {field} '{s}': {e}")))
}

fn side(s: &str) -> Result<Side> {
    match s {
        "bid" | "buy" => Ok(Side::Bid),
        "offer" | "ask" | "sell" => Ok(Side::Ask),
        other => Err(AppError::InvalidField(format!("invalid side '{other}'"))),
    }
}

/// Parse one Advanced Trade WebSocket frame. Every valid JSON object
/// parses; only `l2_data` frames fill [`CoinbaseMessage::events`], and a
/// malformed `l2_data` frame is an error.
pub fn parse_coinbase_message(raw: &str) -> Result<CoinbaseMessage> {
    let env: RawEnvelope = serde_json::from_str(raw)?;
    let mut msg = CoinbaseMessage {
        channel: env.channel,
        sequence_num: env.sequence_num,
        events: Vec::new(),
        error: if env.kind.as_deref() == Some("error") {
            Some(env.message.unwrap_or_default())
        } else {
            None
        },
    };
    if msg.channel.as_deref() == Some("l2_data") {
        if let Some(events) = env.events {
            let events: Vec<RawL2Event> = serde_json::from_str(events.get())?;
            for ev in events {
                let kind = match ev.kind.as_str() {
                    "snapshot" => CoinbaseEventKind::Snapshot,
                    "update" => CoinbaseEventKind::Update,
                    other => {
                        return Err(AppError::InvalidField(format!(
                            "invalid level2 event type '{other}'"
                        )))
                    }
                };
                let updates = ev
                    .updates
                    .iter()
                    .map(|u| {
                        Ok((
                            side(&u.side)?,
                            decimal("price_level", &u.price_level)?,
                            decimal("new_quantity", &u.new_quantity)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                msg.events.push(CoinbaseEvent {
                    kind,
                    product_id: ev.product_id,
                    updates,
                });
            }
        }
    }
    Ok(msg)
}

// ── CoinbaseSync ─────────────────────────────────────────────────────────────

/// Coinbase's `level2` book for one product, as a sans-IO state machine.
///
/// ```
/// use crypto_orderbook::venue::{CoinbaseSync, FeedOutcome, GapReason, VenueSync};
/// use crypto_orderbook::OrderBook;
///
/// let mut book = OrderBook::new();
/// let mut sync = CoinbaseSync::new("BTC-USD");
///
/// // Coinbase's documented snapshot message.
/// let snapshot = r#"{"channel":"l2_data","client_id":"","timestamp":"2023-02-09T20:32:50.714964855Z",
///   "sequence_num":0,"events":[{"type":"snapshot","product_id":"BTC-USD","updates":[
///   {"side":"bid","event_time":"1970-01-01T00:00:00Z","price_level":"21921.73","new_quantity":"0.06317902"},
///   {"side":"bid","event_time":"1970-01-01T00:00:00Z","price_level":"21921.3","new_quantity":"0.02"}]}]}"#;
/// assert_eq!(sync.on_frame(&mut book, snapshot).unwrap(), FeedOutcome::Snapshot);
/// assert_eq!(book.best_bid().unwrap().price.to_string(), "21921.73");
///
/// // A heartbeat on the same connection advances the sequence.
/// let hb = r#"{"channel":"heartbeats","sequence_num":1,"events":[{"heartbeat_counter":"3049"}]}"#;
/// assert_eq!(sync.on_frame(&mut book, hb).unwrap(), FeedOutcome::Ignored);
///
/// // Sequence 2 never arrived: the book is dropped.
/// let update = r#"{"channel":"l2_data","sequence_num":3,"events":[{"type":"update","product_id":"BTC-USD",
///   "updates":[{"side":"offer","event_time":"2023-02-09T20:32:51Z","price_level":"21922.0","new_quantity":"1"}]}]}"#;
/// assert_eq!(
///     sync.on_frame(&mut book, update).unwrap(),
///     FeedOutcome::Resync(GapReason::Sequence { expected: 2, got: 3 })
/// );
/// assert!(sync.needs_snapshot());
/// assert!(book.best_bid().is_none());
/// ```
#[derive(Debug, Clone)]
pub struct CoinbaseSync {
    product_id: String,
    state: StreamState,
    last_sequence: Option<u64>,
    stats: SyncStats,
}

impl CoinbaseSync {
    /// A sync for `product_id` (for example `BTC-USD`), waiting for a snapshot.
    pub fn new(product_id: impl Into<String>) -> Self {
        Self {
            product_id: product_id.into(),
            state: StreamState::AwaitingSnapshot,
            last_sequence: None,
            stats: SyncStats::default(),
        }
    }

    /// The product this sync follows.
    pub fn product_id(&self) -> &str {
        &self.product_id
    }

    /// The current state.
    pub fn state(&self) -> StreamState {
        self.state
    }

    /// The last `sequence_num` accepted on this connection.
    pub fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    /// The subscribe request for this product's `level2` channel. Coinbase
    /// accepts it without a JWT; add a `jwt` field yourself if you have one.
    pub fn subscribe_message(&self) -> String {
        serde_json::json!({"type": "subscribe", "channel": "level2", "product_ids": [self.product_id]}).to_string()
    }

    /// The matching unsubscribe request. Send it and then
    /// [`subscribe_message`](Self::subscribe_message) to get a new snapshot
    /// after a resync without reconnecting.
    pub fn unsubscribe_message(&self) -> String {
        serde_json::json!({"type": "unsubscribe", "channel": "level2", "product_ids": [self.product_id]}).to_string()
    }

    /// Process one parsed frame.
    pub fn on_message(&mut self, book: &mut OrderBook, msg: &CoinbaseMessage) -> FeedOutcome {
        let ours: Vec<&CoinbaseEvent> = msg
            .events
            .iter()
            .filter(|e| e.product_id == self.product_id)
            .collect();
        let has_snapshot = ours.iter().any(|e| e.kind == CoinbaseEventKind::Snapshot);
        if !ours.is_empty() {
            self.stats.events += 1;
        }

        if let Some(seq) = msg.sequence_num {
            if let Some(prev) = self.last_sequence {
                if seq <= prev {
                    // Late or repeated: Coinbase says these can be ignored.
                    if !ours.is_empty() {
                        self.stats.skipped += 1;
                    }
                    return FeedOutcome::Skipped;
                }
                let expected = prev.saturating_add(1);
                if seq != expected && self.state == StreamState::Live && !has_snapshot {
                    // Whatever was lost may have changed this book.
                    self.last_sequence = Some(seq);
                    self.drop_book(book);
                    return FeedOutcome::Resync(GapReason::Sequence { expected, got: seq });
                }
            }
            self.last_sequence = Some(seq);
        }

        let mut out = FeedOutcome::Ignored;
        for event in ours {
            let step = match event.kind {
                CoinbaseEventKind::Snapshot => {
                    book.clear();
                    for &(side, price, qty) in &event.updates {
                        book.set_level(side, price, qty);
                    }
                    self.state = StreamState::Live;
                    self.stats.snapshots += 1;
                    FeedOutcome::Snapshot
                }
                CoinbaseEventKind::Update if self.state == StreamState::Live => {
                    for &(side, price, qty) in &event.updates {
                        book.set_level(side, price, qty);
                    }
                    self.stats.applied += 1;
                    FeedOutcome::Applied
                }
                CoinbaseEventKind::Update => {
                    self.stats.skipped += 1;
                    FeedOutcome::Skipped
                }
            };
            out = merge(out, step);
        }
        if matches!(out, FeedOutcome::Snapshot | FeedOutcome::Applied) {
            if let Some(seq) = msg.sequence_num {
                book.last_update_id = seq;
            }
        }
        out
    }

    fn drop_book(&mut self, book: &mut OrderBook) {
        book.clear();
        self.state = StreamState::AwaitingSnapshot;
        self.stats.resyncs += 1;
    }
}

impl VenueSync for CoinbaseSync {
    fn venue(&self) -> Venue {
        Venue::Coinbase
    }

    fn on_frame(&mut self, book: &mut OrderBook, frame: &str) -> Result<FeedOutcome> {
        let msg = parse_coinbase_message(frame)?;
        Ok(self.on_message(book, &msg))
    }

    fn is_live(&self) -> bool {
        self.state == StreamState::Live
    }

    fn needs_snapshot(&self) -> bool {
        self.state == StreamState::AwaitingSnapshot
    }

    fn stats(&self) -> SyncStats {
        self.stats
    }

    /// Forget the book and the connection's sequence number. Call it after a
    /// reconnect: a new connection starts counting again.
    fn reset(&mut self, book: &mut OrderBook) {
        book.clear();
        self.state = StreamState::AwaitingSnapshot;
        self.last_sequence = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn l2(seq: u64, kind: &str, product: &str, updates: &[(&str, &str, &str)]) -> String {
        let ups: Vec<_> = updates
            .iter()
            .map(|(s, p, q)| serde_json::json!({"side": s, "event_time": "2023-02-09T20:32:50Z", "price_level": p, "new_quantity": q}))
            .collect();
        serde_json::json!({"channel": "l2_data", "client_id": "", "timestamp": "2023-02-09T20:32:50Z",
            "sequence_num": seq, "events": [{"type": kind, "product_id": product, "updates": ups}]})
        .to_string()
    }

    fn hb(seq: u64) -> String {
        format!(
            r#"{{"channel":"heartbeats","client_id":"","sequence_num":{seq},"events":[{{"heartbeat_counter":"1"}}]}}"#
        )
    }

    #[test]
    fn parses_sides_and_quantities() {
        let m = parse_coinbase_message(&l2(
            7,
            "update",
            "ETH-USD",
            &[("offer", "1867.72", "0"), ("bid", "1865.6", "2.5")],
        ))
        .unwrap();
        assert_eq!(m.sequence_num, Some(7));
        assert_eq!(m.events[0].kind, CoinbaseEventKind::Update);
        assert_eq!(
            m.events[0].updates,
            vec![
                (Side::Ask, dec!(1867.72), dec!(0)),
                (Side::Bid, dec!(1865.6), dec!(2.5))
            ]
        );
        assert!(parse_coinbase_message(&l2(1, "update", "X", &[("up", "1", "1")])).is_err());
        assert!(parse_coinbase_message(&l2(1, "update", "X", &[("bid", "one", "1")])).is_err());
        let err =
            parse_coinbase_message(r#"{"type":"error","message":"failure to subscribe"}"#).unwrap();
        assert_eq!(err.error.as_deref(), Some("failure to subscribe"));
    }

    #[test]
    fn updates_before_the_snapshot_are_skipped() {
        let mut book = OrderBook::new();
        let mut sync = CoinbaseSync::new("BTC-USD");
        assert_eq!(
            sync.on_frame(&mut book, &l2(0, "update", "BTC-USD", &[("bid", "1", "1")]))
                .unwrap(),
            FeedOutcome::Skipped
        );
        assert_eq!(book.bid_depth(), 0);
        assert_eq!(
            sync.on_frame(
                &mut book,
                &l2(1, "snapshot", "BTC-USD", &[("bid", "10", "1")])
            )
            .unwrap(),
            FeedOutcome::Snapshot
        );
        assert_eq!(
            sync.on_frame(
                &mut book,
                &l2(
                    2,
                    "update",
                    "BTC-USD",
                    &[("bid", "10", "0"), ("bid", "9", "2")]
                )
            )
            .unwrap(),
            FeedOutcome::Applied
        );
        assert_eq!(book.best_bid().unwrap().price, dec!(9));
        assert_eq!(book.last_update_id, 2);
    }

    #[test]
    fn duplicates_are_skipped_and_other_products_ignored() {
        let mut book = OrderBook::new();
        let mut sync = CoinbaseSync::new("BTC-USD");
        sync.on_frame(
            &mut book,
            &l2(5, "snapshot", "BTC-USD", &[("bid", "10", "1")]),
        )
        .unwrap();
        assert_eq!(
            sync.on_frame(
                &mut book,
                &l2(6, "update", "ETH-USD", &[("bid", "11", "1")])
            )
            .unwrap(),
            FeedOutcome::Ignored
        );
        assert_eq!(
            sync.on_frame(
                &mut book,
                &l2(6, "update", "BTC-USD", &[("bid", "12", "1")])
            )
            .unwrap(),
            FeedOutcome::Skipped
        );
        assert_eq!(
            sync.on_frame(
                &mut book,
                &l2(4, "update", "BTC-USD", &[("bid", "12", "1")])
            )
            .unwrap(),
            FeedOutcome::Skipped
        );
        assert_eq!(book.best_bid().unwrap().price, dec!(10));
        assert_eq!(
            sync.on_frame(&mut book, &hb(7)).unwrap(),
            FeedOutcome::Ignored
        );
        assert!(sync.is_live());
    }

    #[test]
    fn a_gap_in_any_channel_drops_the_book_and_a_new_snapshot_recovers() {
        let mut book = OrderBook::new();
        let mut sync = CoinbaseSync::new("BTC-USD");
        sync.on_frame(
            &mut book,
            &l2(0, "snapshot", "BTC-USD", &[("bid", "10", "1")]),
        )
        .unwrap();
        assert_eq!(
            sync.on_frame(&mut book, &hb(2)).unwrap(),
            FeedOutcome::Resync(GapReason::Sequence {
                expected: 1,
                got: 2
            })
        );
        assert!(sync.needs_snapshot());
        assert_eq!(
            sync.on_frame(
                &mut book,
                &l2(3, "update", "BTC-USD", &[("bid", "11", "1")])
            )
            .unwrap(),
            FeedOutcome::Skipped
        );
        // A gap while waiting for a snapshot is harmless.
        assert_eq!(
            sync.on_frame(
                &mut book,
                &l2(9, "snapshot", "BTC-USD", &[("bid", "20", "1")])
            )
            .unwrap(),
            FeedOutcome::Snapshot
        );
        assert_eq!(book.best_bid().unwrap().price, dec!(20));
        assert_eq!(sync.stats().resyncs, 1);
        assert_eq!(sync.stats().snapshots, 2);
    }

    #[test]
    fn a_gap_on_a_frame_that_carries_a_snapshot_needs_no_resync() {
        let mut book = OrderBook::new();
        let mut sync = CoinbaseSync::new("BTC-USD");
        sync.on_frame(
            &mut book,
            &l2(0, "snapshot", "BTC-USD", &[("bid", "10", "1")]),
        )
        .unwrap();
        assert_eq!(
            sync.on_frame(
                &mut book,
                &l2(5, "snapshot", "BTC-USD", &[("bid", "30", "1")])
            )
            .unwrap(),
            FeedOutcome::Snapshot
        );
        assert_eq!(book.bid_depth(), 1);
        assert_eq!(sync.stats().resyncs, 0);
    }

    #[test]
    fn reset_restarts_the_sequence_for_a_new_connection() {
        let mut book = OrderBook::new();
        let mut sync = CoinbaseSync::new("BTC-USD");
        sync.on_frame(
            &mut book,
            &l2(40, "snapshot", "BTC-USD", &[("bid", "10", "1")]),
        )
        .unwrap();
        sync.reset(&mut book);
        assert_eq!(sync.last_sequence(), None);
        assert_eq!(
            sync.on_frame(
                &mut book,
                &l2(0, "snapshot", "BTC-USD", &[("bid", "10", "1")])
            )
            .unwrap(),
            FeedOutcome::Snapshot
        );
    }

    #[test]
    fn subscribe_messages() {
        let sync = CoinbaseSync::new("BTC-USD");
        let v: serde_json::Value = serde_json::from_str(&sync.subscribe_message()).unwrap();
        assert_eq!(v["channel"], "level2");
        assert_eq!(v["type"], "subscribe");
        assert_eq!(v["product_ids"][0], "BTC-USD");
        assert!(sync.unsubscribe_message().contains("unsubscribe"));
    }
}
