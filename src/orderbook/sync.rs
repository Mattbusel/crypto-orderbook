//! Binance's rules for stitching a REST depth snapshot to the WebSocket diff
//! stream: the pure decision functions [`handshake`] and [`live`], and
//! [`BookSync`], a sans-IO state machine that runs the whole procedure
//! (buffer, snapshot, drop stale events, connect, follow, resync) on top of
//! an [`OrderBook`]. The service, the offline replay and library users all
//! go through [`BookSync`], so there is one implementation of the rules.
//!
//! Spot and USD-M futures use different sequencing:
//!
//! - **Spot** ([docs](https://developers.binance.com/docs/binance-spot-api-docs/web-socket-streams#how-to-manage-a-local-order-book-correctly)):
//!   buffer events and note the first one's `U`; refetch the snapshot while
//!   it is older than that event; drop events with `u <= lastUpdateId`; the
//!   first applied event has `U <= lastUpdateId + 1 <= u`. Afterwards ignore
//!   events with `u <=` the local update id and resync if `U > local + 1`, so
//!   a clean stream continues with `U == previous u + 1`.
//! - **USD-M futures** ([docs](https://developers.binance.com/docs/derivatives/usds-margined-futures/websocket-market-streams/How-to-manage-a-local-order-book-correctly)):
//!   drop events with `u < lastUpdateId`; the first applied event has
//!   `U <= lastUpdateId <= u`. Afterwards each event's `pu` must equal the
//!   previous event's `u` (futures `U` values are not contiguous).
//!
//! Binance's current spot page words step 5 as "refetch if `lastUpdateId <
//! U`" and step 6 as "`lastUpdateId` within `[U; u]`". Read literally that
//! rejects a snapshot whose id is exactly `U - 1`, which connects with no
//! missing update (spot ids are contiguous). This module accepts it, as
//! Binance's earlier wording (`U <= lastUpdateId + 1`) did; that saves one
//! snapshot request (up to 250 weight) and is covered by property tests that
//! check the resulting book against a reference model.

use std::collections::VecDeque;

use crate::orderbook::book::OrderBook;
use crate::ws::binance::{DepthSnapshot, DepthUpdate};

/// Which Binance market the book follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Market {
    /// Spot (`api.binance.com`, `stream.binance.com`).
    Spot,
    /// USD-M perpetual futures (`fapi.binance.com`, `fstream.binance.com`).
    UsdM,
}

impl Market {
    /// Parse `spot` or `usdm` (case-insensitive; `futures` is accepted for `usdm`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "spot" => Some(Self::Spot),
            "usdm" | "usd-m" | "futures" => Some(Self::UsdM),
            _ => None,
        }
    }

    /// Lower-case name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spot => "spot",
            Self::UsdM => "usdm",
        }
    }

    /// Default REST base URL.
    pub fn default_rest_base(self) -> &'static str {
        match self {
            Self::Spot => "https://api.binance.com",
            Self::UsdM => "https://fapi.binance.com",
        }
    }

    /// Path of the depth snapshot endpoint.
    pub fn depth_path(self) -> &'static str {
        match self {
            Self::Spot => "/api/v3/depth",
            Self::UsdM => "/fapi/v1/depth",
        }
    }

    /// Largest snapshot the endpoint serves.
    pub fn max_depth_limit(self) -> u32 {
        match self {
            Self::Spot => 5000,
            Self::UsdM => 1000,
        }
    }

    /// Default diff-depth WebSocket URL for `symbol`.
    pub fn default_ws_url(self, symbol: &str) -> String {
        let s = symbol.to_ascii_lowercase();
        match self {
            Self::Spot => format!("wss://stream.binance.com:9443/ws/{s}@depth"),
            Self::UsdM => format!("wss://fstream.binance.com/ws/{s}@depth@100ms"),
        }
    }

    /// Default trade WebSocket URL (for the VWAP) for `symbol`. Futures
    /// publish aggregated trades, which carry the same price, quantity and
    /// time fields.
    pub fn default_trade_ws_url(self, symbol: &str) -> String {
        let s = symbol.to_ascii_lowercase();
        match self {
            Self::Spot => format!("wss://stream.binance.com:9443/ws/{s}@trade"),
            Self::UsdM => format!("wss://fstream.binance.com/ws/{s}@aggTrade"),
        }
    }
}

/// What to do with a buffered event while connecting to a fresh snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handshake {
    /// Already contained in the snapshot: drop it.
    Stale,
    /// The first event to apply on top of the snapshot.
    Apply,
    /// Events between the snapshot and this one were missed: fetch a new snapshot.
    Gap,
}

/// Decide what to do with `update` while connecting to a snapshot taken at
/// `snapshot_id` (its `lastUpdateId`).
pub fn handshake(market: Market, snapshot_id: u64, update: &DepthUpdate) -> Handshake {
    match market {
        Market::Spot => {
            if update.last_update_id <= snapshot_id {
                Handshake::Stale
            } else if update.first_update_id > snapshot_id.saturating_add(1) {
                Handshake::Gap
            } else {
                Handshake::Apply
            }
        }
        Market::UsdM => {
            if update.last_update_id < snapshot_id {
                Handshake::Stale
            } else if update.first_update_id > snapshot_id {
                Handshake::Gap
            } else {
                Handshake::Apply
            }
        }
    }
}

/// What to do with a live event, given the `u` of the last applied event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Live {
    /// Continues the sequence: apply it.
    Apply,
    /// Already applied (a duplicate or late event): skip it without resyncing.
    Ignore,
    /// An update was missed: the book is wrong until a resync.
    Gap {
        /// The id the next event had to start from (spot) or point back to (futures).
        expected: u64,
        /// What the event carried.
        got: u64,
    },
}

/// Decide what to do with a live `update` after the event whose `u` was `prev_last`.
pub fn live(market: Market, prev_last: u64, update: &DepthUpdate) -> Live {
    match market {
        Market::Spot => {
            if update.last_update_id <= prev_last {
                Live::Ignore
            } else if update.first_update_id > prev_last.saturating_add(1) {
                Live::Gap {
                    expected: prev_last.saturating_add(1),
                    got: update.first_update_id,
                }
            } else {
                Live::Apply
            }
        }
        Market::UsdM => {
            if update.last_update_id <= prev_last {
                Live::Ignore
            } else {
                match update.prev_last_update_id {
                    Some(pu) if pu == prev_last => Live::Apply,
                    Some(pu) => Live::Gap {
                        expected: prev_last,
                        got: pu,
                    },
                    // A futures event without `pu` cannot be checked: treat it
                    // like a gap rather than risk a silently wrong book.
                    None => Live::Gap {
                        expected: prev_last,
                        got: 0,
                    },
                }
            }
        }
    }
}

/// Whether `snapshot_id` is too old to connect to a stream whose earliest
/// buffered event starts at `first_update_id` (`U`). Binance's step "if the
/// snapshot is older than the first buffered event, fetch it again".
pub fn snapshot_is_too_old(market: Market, snapshot_id: u64, first_update_id: u64) -> bool {
    match market {
        Market::Spot => first_update_id > snapshot_id.saturating_add(1),
        Market::UsdM => first_update_id > snapshot_id,
    }
}

// ── BookSync: the whole procedure as a state machine ──────────────────────────

/// Where a [`BookSync`] is in Binance's procedure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// No usable snapshot: events are buffered until one is supplied.
    Buffering,
    /// A snapshot is applied; waiting for the first event that connects to it.
    Handshake {
        /// The snapshot's `lastUpdateId`.
        snapshot_id: u64,
    },
    /// Following the stream; the book is correct as of `last_update_id`.
    Live {
        /// The `u` of the last applied event.
        last_update_id: u64,
    },
}

/// Why a [`BookSync`] dropped its book and went back to buffering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResyncReason {
    /// The first event after the snapshot starts past it: updates between
    /// the two were never seen. Fetch a newer snapshot.
    SnapshotTooOld {
        /// The snapshot's `lastUpdateId`.
        snapshot_id: u64,
        /// The event's `U`.
        first_update_id: u64,
    },
    /// An update was missed while following the stream.
    Gap {
        /// The id the event had to start from (spot `U`) or point back to (futures `pu`).
        expected: u64,
        /// What the event carried (`0` for a futures event without `pu`).
        got: u64,
    },
}

/// What a [`BookSync`] did with one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// No snapshot yet: held until [`BookSync::on_snapshot`].
    Buffered,
    /// Applied to the book.
    Applied,
    /// Already contained in the snapshot or already applied: dropped.
    Skipped,
    /// The book can no longer be trusted. It has been cleared, the event is
    /// kept as the first buffered event, and a new snapshot is needed.
    Resync(ResyncReason),
}

/// What happened when a snapshot was supplied and the buffer replayed onto it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SnapshotOutcome {
    /// Buffered events applied on top of the snapshot.
    pub applied: usize,
    /// Buffered events dropped as already contained in the snapshot.
    pub skipped: usize,
    /// Set if the snapshot did not connect to the buffered events.
    pub resync: Option<ResyncReason>,
}

/// Running counts kept by a [`BookSync`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncStats {
    /// Events passed to [`BookSync::on_event`].
    pub events: u64,
    /// Events applied to the book (including buffered ones applied later).
    pub applied: u64,
    /// Events dropped as stale or duplicate.
    pub skipped: u64,
    /// Snapshots supplied.
    pub snapshots: u64,
    /// Times the book was dropped because of a gap or a too-old snapshot.
    pub resyncs: u64,
    /// Buffered events discarded because the buffer was full.
    pub buffer_overflows: u64,
}

/// Binance's local order book procedure as a sans-IO state machine.
///
/// It does no networking: feed it every parsed diff-depth event with
/// [`on_event`](Self::on_event) and a REST snapshot with
/// [`on_snapshot`](Self::on_snapshot) whenever [`needs_snapshot`](Self::needs_snapshot)
/// says so, and it keeps `book` correct or tells you it cannot.
///
/// 1. Events that arrive before a snapshot are buffered.
/// 2. If [`snapshot_is_too_old`](Self::snapshot_is_too_old) says the snapshot
///    predates the first buffered event, fetch another.
/// 3. The snapshot replaces the book; buffered events already inside it are
///    dropped and the first one that connects is applied.
/// 4. Every later event must continue the sequence (spot `U == u + 1` or
///    overlapping, futures `pu == u`); duplicates are skipped.
/// 5. On a gap the book is cleared and the procedure starts again from 1,
///    keeping the event that revealed the gap.
///
/// ```
/// use crypto_orderbook::{parse_depth_snapshot, parse_depth_update, BookSync, Market, OrderBook, Outcome};
///
/// let mut book = OrderBook::new();
/// let mut sync = BookSync::new(Market::Spot);
///
/// // The stream starts first; its events are buffered.
/// let ev = parse_depth_update(r#"{"e":"depthUpdate","E":1,"s":"BTCUSDT","U":99,"u":102,"b":[["100.5","2"]],"a":[]}"#).unwrap();
/// assert_eq!(sync.on_event(&mut book, ev), Outcome::Buffered);
///
/// // Then the REST snapshot; the buffered event overlaps it and is applied.
/// let snap = parse_depth_snapshot(r#"{"lastUpdateId":100,"bids":[["100.0","1"]],"asks":[["101.0","1"]]}"#).unwrap();
/// assert!(!sync.snapshot_is_too_old(snap.last_update_id));
/// let done = sync.on_snapshot(&mut book, snap);
/// assert_eq!((done.applied, done.resync), (1, None));
/// assert!(sync.is_live());
/// assert_eq!(book.best_bid().unwrap().price.to_string(), "100.5");
///
/// // A missed event is detected and the book is dropped.
/// let late = parse_depth_update(r#"{"e":"depthUpdate","E":2,"s":"BTCUSDT","U":110,"u":111,"b":[],"a":[]}"#).unwrap();
/// assert!(matches!(sync.on_event(&mut book, late), Outcome::Resync(_)));
/// assert!(sync.needs_snapshot());
/// assert!(book.best_bid().is_none());
/// ```
#[derive(Debug, Clone)]
pub struct BookSync {
    market: Market,
    state: SyncState,
    buffer: VecDeque<DepthUpdate>,
    max_buffer: usize,
    stats: SyncStats,
}

impl BookSync {
    /// Default cap on buffered events.
    pub const DEFAULT_MAX_BUFFER: usize = 10_000;

    /// A state machine for `market`, starting in [`SyncState::Buffering`].
    pub fn new(market: Market) -> Self {
        Self {
            market,
            state: SyncState::Buffering,
            buffer: VecDeque::new(),
            max_buffer: Self::DEFAULT_MAX_BUFFER,
            stats: SyncStats::default(),
        }
    }

    /// Keep at most `max` events while waiting for a snapshot (minimum 1).
    /// When full, the oldest is discarded; that only ever makes a snapshot
    /// look too old, never lets a gap through.
    pub fn with_max_buffer(mut self, max: usize) -> Self {
        self.max_buffer = max.max(1);
        self
    }

    /// The market whose rules are applied.
    pub fn market(&self) -> Market {
        self.market
    }

    /// The current state.
    pub fn state(&self) -> SyncState {
        self.state
    }

    /// True once an event has connected to the snapshot and the book is
    /// being kept current.
    pub fn is_live(&self) -> bool {
        matches!(self.state, SyncState::Live { .. })
    }

    /// True when the caller should fetch a REST snapshot: no snapshot is in
    /// use and the stream has delivered at least one event (Binance's order:
    /// stream first, then snapshot, so the snapshot is not older than it).
    pub fn needs_snapshot(&self) -> bool {
        self.state == SyncState::Buffering && !self.buffer.is_empty()
    }

    /// Events held for the next snapshot.
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }

    /// The `U` of the earliest buffered event.
    pub fn first_buffered_update_id(&self) -> Option<u64> {
        self.buffer.front().map(|u| u.first_update_id)
    }

    /// Whether a snapshot with this `lastUpdateId` is too old for the
    /// buffered events, in which case fetch another before calling
    /// [`on_snapshot`](Self::on_snapshot). False if nothing is buffered.
    pub fn snapshot_is_too_old(&self, snapshot_id: u64) -> bool {
        self.first_buffered_update_id()
            .is_some_and(|first| snapshot_is_too_old(self.market, snapshot_id, first))
    }

    /// Running counts.
    pub fn stats(&self) -> SyncStats {
        self.stats
    }

    /// Forget everything (call it when the WebSocket reconnects: events from
    /// the old connection cannot be stitched to the new one).
    pub fn reset(&mut self, book: &mut OrderBook) {
        book.clear();
        self.buffer.clear();
        self.state = SyncState::Buffering;
    }

    /// Process one diff-depth event.
    pub fn on_event(&mut self, book: &mut OrderBook, update: DepthUpdate) -> Outcome {
        self.stats.events += 1;
        self.step(book, update)
    }

    /// Replace the book with `snapshot` and replay the buffered events onto it.
    pub fn on_snapshot(
        &mut self,
        book: &mut OrderBook,
        snapshot: DepthSnapshot,
    ) -> SnapshotOutcome {
        self.stats.snapshots += 1;
        let snapshot_id = snapshot.last_update_id;
        book.clear();
        book.apply(&DepthUpdate {
            first_update_id: snapshot_id,
            last_update_id: snapshot_id,
            prev_last_update_id: None,
            bids: snapshot.bids,
            asks: snapshot.asks,
        });
        self.state = SyncState::Handshake { snapshot_id };

        let mut out = SnapshotOutcome::default();
        for update in std::mem::take(&mut self.buffer) {
            match self.step(book, update) {
                Outcome::Applied => out.applied += 1,
                Outcome::Skipped => out.skipped += 1,
                Outcome::Resync(reason) => out.resync = Some(reason),
                // After a resync the rest goes back into the buffer.
                Outcome::Buffered => {}
            }
        }
        out
    }

    fn buffer_push(&mut self, update: DepthUpdate) {
        if self.buffer.len() >= self.max_buffer {
            self.buffer.pop_front();
            self.stats.buffer_overflows += 1;
        }
        self.buffer.push_back(update);
    }

    fn resync(
        &mut self,
        book: &mut OrderBook,
        update: DepthUpdate,
        reason: ResyncReason,
    ) -> Outcome {
        book.clear();
        self.buffer.clear();
        self.stats.resyncs += 1;
        self.state = SyncState::Buffering;
        // The event that revealed the gap is the earliest one the next
        // snapshot has to connect to.
        self.buffer_push(update);
        Outcome::Resync(reason)
    }

    fn step(&mut self, book: &mut OrderBook, update: DepthUpdate) -> Outcome {
        match self.state {
            SyncState::Buffering => {
                self.buffer_push(update);
                Outcome::Buffered
            }
            SyncState::Handshake { snapshot_id } => {
                match handshake(self.market, snapshot_id, &update) {
                    Handshake::Stale => {
                        self.stats.skipped += 1;
                        Outcome::Skipped
                    }
                    Handshake::Gap => {
                        let reason = ResyncReason::SnapshotTooOld {
                            snapshot_id,
                            first_update_id: update.first_update_id,
                        };
                        self.resync(book, update, reason)
                    }
                    Handshake::Apply => self.apply(book, &update),
                }
            }
            SyncState::Live { last_update_id } => {
                match live(self.market, last_update_id, &update) {
                    Live::Apply => self.apply(book, &update),
                    Live::Ignore => {
                        self.stats.skipped += 1;
                        Outcome::Skipped
                    }
                    Live::Gap { expected, got } => {
                        self.resync(book, update, ResyncReason::Gap { expected, got })
                    }
                }
            }
        }
    }

    fn apply(&mut self, book: &mut OrderBook, update: &DepthUpdate) -> Outcome {
        book.apply(update);
        self.stats.applied += 1;
        self.state = SyncState::Live {
            last_update_id: update.last_update_id,
        };
        Outcome::Applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(first: u64, last: u64, pu: Option<u64>) -> DepthUpdate {
        DepthUpdate {
            first_update_id: first,
            last_update_id: last,
            prev_last_update_id: pu,
            bids: vec![],
            asks: vec![],
        }
    }

    // ── spot handshake ──

    #[test]
    fn spot_drops_events_inside_the_snapshot() {
        assert_eq!(
            handshake(Market::Spot, 100, &ev(90, 100, None)),
            Handshake::Stale
        );
    }

    #[test]
    fn spot_applies_an_event_that_overlaps_or_follows_the_snapshot() {
        assert_eq!(
            handshake(Market::Spot, 100, &ev(95, 105, None)),
            Handshake::Apply
        );
        assert_eq!(
            handshake(Market::Spot, 100, &ev(101, 105, None)),
            Handshake::Apply
        );
    }

    #[test]
    fn spot_detects_a_gap_after_the_snapshot() {
        assert_eq!(
            handshake(Market::Spot, 100, &ev(102, 105, None)),
            Handshake::Gap
        );
    }

    // ── spot live ──

    #[test]
    fn spot_live_applies_the_next_event() {
        assert_eq!(live(Market::Spot, 105, &ev(106, 110, None)), Live::Apply);
    }

    #[test]
    fn spot_live_ignores_a_duplicate_instead_of_resyncing() {
        // Re-delivered or late events are harmless; resyncing on them costs a
        // snapshot (weight 250 at limit 5000) for nothing.
        assert_eq!(live(Market::Spot, 110, &ev(106, 110, None)), Live::Ignore);
        assert_eq!(live(Market::Spot, 110, &ev(100, 104, None)), Live::Ignore);
    }

    #[test]
    fn spot_live_detects_a_missed_event() {
        assert_eq!(
            live(Market::Spot, 105, &ev(107, 110, None)),
            Live::Gap {
                expected: 106,
                got: 107
            }
        );
    }

    // ── futures handshake ──

    #[test]
    fn futures_drops_events_strictly_before_the_snapshot() {
        assert_eq!(
            handshake(Market::UsdM, 100, &ev(90, 99, Some(89))),
            Handshake::Stale
        );
    }

    #[test]
    fn futures_applies_the_event_that_covers_the_snapshot_id() {
        // U <= lastUpdateId <= u. Note u == lastUpdateId is NOT stale on futures.
        assert_eq!(
            handshake(Market::UsdM, 100, &ev(95, 100, Some(94))),
            Handshake::Apply
        );
        assert_eq!(
            handshake(Market::UsdM, 100, &ev(98, 103, Some(97))),
            Handshake::Apply
        );
    }

    #[test]
    fn futures_detects_a_gap_after_the_snapshot() {
        // Spot would accept U == lastUpdateId + 1; futures requires U <= lastUpdateId.
        assert_eq!(
            handshake(Market::UsdM, 100, &ev(101, 105, Some(100))),
            Handshake::Gap
        );
    }

    // ── futures live ──

    #[test]
    fn futures_live_follows_pu_not_u_plus_one() {
        // Futures ids are not contiguous: the next event's U can jump, but its
        // pu must equal the previous u.
        assert_eq!(
            live(Market::UsdM, 105, &ev(120, 130, Some(105))),
            Live::Apply
        );
        assert_eq!(
            live(Market::UsdM, 105, &ev(106, 110, Some(104))),
            Live::Gap {
                expected: 105,
                got: 104
            }
        );
    }

    #[test]
    fn futures_live_without_pu_is_treated_as_a_gap() {
        assert!(matches!(
            live(Market::UsdM, 105, &ev(106, 110, None)),
            Live::Gap { .. }
        ));
    }

    #[test]
    fn market_names_parse() {
        assert_eq!(Market::parse("SPOT"), Some(Market::Spot));
        assert_eq!(Market::parse("usdm"), Some(Market::UsdM));
        assert_eq!(Market::parse("futures"), Some(Market::UsdM));
        assert_eq!(Market::parse("coinm"), None);
    }

    // ── snapshot age, at the exact boundaries ──

    #[test]
    fn spot_snapshot_one_below_the_first_event_still_connects() {
        // Spot ids are contiguous: a snapshot at U - 1 is followed by U.
        assert!(!snapshot_is_too_old(Market::Spot, 99, 100));
        assert!(snapshot_is_too_old(Market::Spot, 98, 100));
        assert_eq!(
            handshake(Market::Spot, 99, &ev(100, 105, None)),
            Handshake::Apply
        );
    }

    #[test]
    fn futures_snapshot_must_reach_the_first_event() {
        assert!(!snapshot_is_too_old(Market::UsdM, 100, 100));
        assert!(snapshot_is_too_old(Market::UsdM, 99, 100));
    }

    #[test]
    fn ids_near_u64_max_do_not_overflow() {
        assert_eq!(
            handshake(Market::Spot, u64::MAX, &ev(u64::MAX, u64::MAX, None)),
            Handshake::Stale
        );
        assert_eq!(
            live(Market::Spot, u64::MAX - 1, &ev(u64::MAX, u64::MAX, None)),
            Live::Apply
        );
        assert!(!snapshot_is_too_old(Market::Spot, u64::MAX, u64::MAX));
    }

    #[test]
    fn book_sync_follows_a_futures_stream_through_a_resync() {
        let mut book = OrderBook::new();
        let mut sync = BookSync::new(Market::UsdM);
        assert_eq!(
            sync.on_event(&mut book, ev(90, 98, Some(80))),
            Outcome::Buffered
        );
        assert_eq!(
            sync.on_event(&mut book, ev(99, 104, Some(98))),
            Outcome::Buffered
        );
        let done = sync.on_snapshot(
            &mut book,
            DepthSnapshot {
                last_update_id: 100,
                bids: vec![],
                asks: vec![],
            },
        );
        assert_eq!((done.skipped, done.applied, done.resync), (1, 1, None));
        assert_eq!(
            sync.state(),
            SyncState::Live {
                last_update_id: 104
            }
        );
        assert_eq!(
            sync.on_event(&mut book, ev(110, 120, Some(104))),
            Outcome::Applied
        );
        assert_eq!(
            sync.on_event(&mut book, ev(110, 120, Some(104))),
            Outcome::Skipped
        );
        assert_eq!(
            sync.on_event(&mut book, ev(130, 140, Some(125))),
            Outcome::Resync(ResyncReason::Gap {
                expected: 120,
                got: 125
            })
        );
        assert!(sync.needs_snapshot());
        assert_eq!(sync.first_buffered_update_id(), Some(130));
        assert_eq!(
            sync.stats(),
            SyncStats {
                events: 5,
                applied: 2,
                skipped: 2,
                snapshots: 1,
                resyncs: 1,
                buffer_overflows: 0
            }
        );
    }
}
