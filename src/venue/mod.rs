//! One API across venues: Binance (spot and USD-M futures), Coinbase
//! Advanced Trade `level2` and Kraken WebSocket v2 `book`.
//!
//! Every venue gets the same sans-IO treatment as Binance's [`BookSync`]:
//! you own the sockets, the sync owns the rules. Each sync keeps an
//! [`OrderBook`] correct or tells you it cannot, with
//! [`FeedOutcome::Resync`] and a [`GapReason`] saying what was missed.
//!
//! | Venue | Where the snapshot comes from | How a gap is detected |
//! |---|---|---|
//! | Binance | REST, stitched to the stream by update id | `U`/`u` (spot) or `pu` (USD-M) |
//! | Coinbase | in the stream, on subscribe | per-connection `sequence_num` must grow by exactly 1 |
//! | Kraken | in the stream, on subscribe | CRC32 checksum of the top 10 levels after every message |
//!
//! Two traits make the venues interchangeable:
//!
//! - [`Book`]: read a book (best bid and ask, top levels, spread, mid,
//!   micro-price, imbalance) without caring which venue filled it.
//!   [`OrderBook`] implements it.
//! - [`VenueSync`]: feed raw text frames (`on_frame`) and ask whether the
//!   book is live, whether it needs a snapshot and where that snapshot comes
//!   from ([`SnapshotSource`]). [`BookSync`], [`CoinbaseSync`] and
//!   [`KrakenSync`] implement it.
//!
//! ```
//! use crypto_orderbook::venue::{Book, FeedOutcome, KrakenSync, VenueSync};
//! use crypto_orderbook::OrderBook;
//!
//! // Kraken's own example snapshot (docs.kraken.com, book checksum guide).
//! let frame = r#"{"channel":"book","type":"snapshot","data":[{"symbol":"BTC/USD",
//!   "bids":[{"price":"45283.5","qty":"0.10000000"},{"price":"45283.4","qty":"1.54582015"},
//!   {"price":"45282.1","qty":"0.10000000"},{"price":"45281.0","qty":"0.10000000"},
//!   {"price":"45280.3","qty":"1.54592586"},{"price":"45279.0","qty":"0.07990000"},
//!   {"price":"45277.6","qty":"0.03310103"},{"price":"45277.5","qty":"0.30000000"},
//!   {"price":"45277.3","qty":"1.54602737"},{"price":"45276.6","qty":"0.15445238"}],
//!   "asks":[{"price":"45285.2","qty":"0.00100000"},{"price":"45286.4","qty":"1.54571953"},
//!   {"price":"45286.6","qty":"1.54571109"},{"price":"45289.6","qty":"1.54560911"},
//!   {"price":"45290.2","qty":"0.15890660"},{"price":"45291.8","qty":"1.54553491"},
//!   {"price":"45294.7","qty":"0.04454749"},{"price":"45296.1","qty":"0.35380000"},
//!   {"price":"45297.5","qty":"0.09945542"},{"price":"45299.5","qty":"0.18772827"}],
//!   "checksum":3310070434}]}"#;
//!
//! let mut book = OrderBook::new();
//! let mut sync: Box<dyn VenueSync> = Box::new(KrakenSync::new("BTC/USD", 10, 1, 8));
//! assert_eq!(sync.on_frame(&mut book, frame).unwrap(), FeedOutcome::Snapshot);
//! assert!(sync.is_live());
//!
//! // Any venue's book reads the same way.
//! fn describe(book: &impl Book) -> String {
//!     format!("{} / {}", book.best_bid().unwrap().price, book.best_ask().unwrap().price)
//! }
//! assert_eq!(describe(&book), "45283.5 / 45285.2");
//! assert_eq!(book.mid_price().unwrap().to_string(), "45284.35");
//! ```

// The crate-wide `AppError` is large (it can hold a WebSocket error); the
// parsers return it like the Binance ones do.
#![allow(clippy::result_large_err)]

pub mod coinbase;
pub mod kraken;

use rust_decimal::Decimal;
use serde::Deserialize;

use crate::error::Result;
use crate::orderbook::book::{OrderBook, PriceLevel};
use crate::orderbook::sync::{BookSync, Market, Outcome, ResyncReason, SyncStats};
use crate::ws::binance::{parse_depth_snapshot, parse_depth_update};

pub use coinbase::{
    parse_coinbase_message, CoinbaseEvent, CoinbaseEventKind, CoinbaseMessage, CoinbaseSync,
};
pub use kraken::{
    kraken_checksum, kraken_checksum_input, parse_kraken_instruments, parse_kraken_message,
    KrakenBookData, KrakenMessage, KrakenPrecision, KrakenSync,
};

// ── Venue ────────────────────────────────────────────────────────────────────

/// An exchange (and, for Binance, which market) a book follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Venue {
    /// Binance spot or USD-M futures.
    Binance(Market),
    /// Coinbase Advanced Trade (`level2` channel).
    Coinbase,
    /// Kraken spot, WebSocket v2 (`book` channel).
    Kraken,
}

impl Venue {
    /// Parse `binance` / `binance-spot`, `binance-usdm` (or `binance-futures`),
    /// `coinbase` and `kraken` (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "binance" | "binance-spot" | "spot" => Some(Self::Binance(Market::Spot)),
            "binance-usdm" | "binance-futures" | "usdm" => Some(Self::Binance(Market::UsdM)),
            "coinbase" | "coinbase-advanced" => Some(Self::Coinbase),
            "kraken" => Some(Self::Kraken),
            _ => None,
        }
    }

    /// Lower-case name (`binance-spot`, `binance-usdm`, `coinbase`, `kraken`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Binance(Market::Spot) => "binance-spot",
            Self::Binance(Market::UsdM) => "binance-usdm",
            Self::Coinbase => "coinbase",
            Self::Kraken => "kraken",
        }
    }

    /// Where this venue's snapshots come from.
    pub fn snapshot_source(self) -> SnapshotSource {
        match self {
            Self::Binance(_) => SnapshotSource::Rest,
            Self::Coinbase | Self::Kraken => SnapshotSource::Stream,
        }
    }

    /// Default public WebSocket URL. For Binance it is the diff-depth stream
    /// of `symbol`; Coinbase and Kraken take the symbol in the subscribe
    /// message instead (see [`CoinbaseSync::subscribe_message`] and
    /// [`KrakenSync::subscribe_message`]).
    pub fn default_ws_url(self, symbol: &str) -> String {
        match self {
            Self::Binance(market) => market.default_ws_url(symbol),
            Self::Coinbase => coinbase::WS_URL.to_string(),
            Self::Kraken => kraken::WS_URL.to_string(),
        }
    }
}

impl std::fmt::Display for Venue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a venue delivers the snapshot a sync needs after a gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotSource {
    /// Fetch it over REST and pass the body to [`VenueSync::on_frame`]
    /// (Binance).
    Rest,
    /// The venue sends one in the stream when you subscribe: unsubscribe and
    /// subscribe again (or reconnect, then call [`VenueSync::reset`]) and
    /// keep feeding frames (Coinbase, Kraken).
    Stream,
}

// ── Outcomes ─────────────────────────────────────────────────────────────────

/// Why a sync dropped its book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GapReason {
    /// Binance's update-id rules found a gap or a too-old snapshot.
    Binance(ResyncReason),
    /// A message sequence number skipped ahead (Coinbase): messages between
    /// the two were lost.
    Sequence {
        /// The sequence number the message had to carry.
        expected: u64,
        /// What it carried.
        got: u64,
    },
    /// The book's checksum disagrees with the venue's (Kraken).
    Checksum {
        /// The checksum the venue sent.
        expected: u32,
        /// The checksum of the local book after applying the message.
        computed: u32,
    },
}

/// What a sync did with one frame or message, on any venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FeedOutcome {
    /// The frame carried no book data for this sync (a heartbeat, a
    /// subscription acknowledgement, another symbol or channel). A Coinbase
    /// frame still advances the connection's sequence number.
    Ignored,
    /// Held until a snapshot arrives (Binance, before its REST snapshot).
    Buffered,
    /// A snapshot replaced the book and it is now live.
    Snapshot,
    /// Applied to the book.
    Applied,
    /// Dropped without harm: already applied, a duplicate, or an update that
    /// arrived while waiting for a snapshot.
    Skipped,
    /// The book can no longer be trusted and has been cleared. Get a new
    /// snapshot (see [`VenueSync::snapshot_source`]).
    Resync(GapReason),
}

impl FeedOutcome {
    /// True for [`FeedOutcome::Resync`].
    pub fn is_resync(&self) -> bool {
        matches!(self, Self::Resync(_))
    }
}

impl From<Outcome> for FeedOutcome {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Buffered => Self::Buffered,
            Outcome::Applied => Self::Applied,
            Outcome::Skipped => Self::Skipped,
            Outcome::Resync(reason) => Self::Resync(GapReason::Binance(reason)),
        }
    }
}

// ── Book: the read side, venue-neutral ───────────────────────────────────────

/// Read access to an order book, whichever venue it follows.
///
/// Only the six level queries are required; spread, mid, micro-price and
/// imbalance are derived from them. [`OrderBook`] implements it, so code
/// written against `impl Book` works for Binance, Coinbase and Kraken books
/// alike, and for your own book types.
pub trait Book {
    /// The highest bid, if any.
    fn best_bid(&self) -> Option<PriceLevel>;
    /// The lowest ask, if any.
    fn best_ask(&self) -> Option<PriceLevel>;
    /// The best `n` bids, highest price first.
    fn top_bids(&self, n: usize) -> Vec<PriceLevel>;
    /// The best `n` asks, lowest price first.
    fn top_asks(&self, n: usize) -> Vec<PriceLevel>;
    /// Number of bid levels.
    fn bid_depth(&self) -> usize;
    /// Number of ask levels.
    fn ask_depth(&self) -> usize;

    /// Best ask minus best bid.
    fn spread(&self) -> Option<Decimal> {
        Some(self.best_ask()?.price - self.best_bid()?.price)
    }

    /// Midpoint of the best bid and ask, without trailing zeros.
    fn mid_price(&self) -> Option<Decimal> {
        Some(((self.best_ask()?.price + self.best_bid()?.price) / Decimal::TWO).normalize())
    }

    /// Liquidity-weighted mid-price:
    /// `(ask * bid_qty + bid * ask_qty) / (bid_qty + ask_qty)`.
    fn micro_price(&self) -> Option<Decimal> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        let total = bid.quantity + ask.quantity;
        if total.is_zero() {
            return None;
        }
        Some((ask.price * bid.quantity + bid.price * ask.quantity) / total)
    }

    /// `(bid_qty - ask_qty) / (bid_qty + ask_qty)` over the top `n` levels.
    fn depth_imbalance(&self, n: usize) -> Option<Decimal> {
        let bid: Decimal = self.top_bids(n).iter().map(|l| l.quantity).sum();
        let ask: Decimal = self.top_asks(n).iter().map(|l| l.quantity).sum();
        let total = bid + ask;
        if total.is_zero() {
            return None;
        }
        Some((bid - ask) / total)
    }

    /// True if the best bid is at or above the best ask, which a correct
    /// book never shows.
    fn is_crossed(&self) -> bool {
        matches!((self.best_bid(), self.best_ask()), (Some(b), Some(a)) if b.price >= a.price)
    }
}

impl Book for OrderBook {
    fn best_bid(&self) -> Option<PriceLevel> {
        OrderBook::best_bid(self)
    }
    fn best_ask(&self) -> Option<PriceLevel> {
        OrderBook::best_ask(self)
    }
    fn top_bids(&self, n: usize) -> Vec<PriceLevel> {
        OrderBook::top_bids(self, n)
    }
    fn top_asks(&self, n: usize) -> Vec<PriceLevel> {
        OrderBook::top_asks(self, n)
    }
    fn bid_depth(&self) -> usize {
        OrderBook::bid_depth(self)
    }
    fn ask_depth(&self) -> usize {
        OrderBook::ask_depth(self)
    }
}

// ── VenueSync: the write side, venue-neutral ─────────────────────────────────

/// A sans-IO book synchroniser for one venue and symbol.
///
/// Feed it every text frame the connection delivers (and, for Binance, the
/// REST snapshot body) with [`on_frame`](Self::on_frame). After a
/// [`FeedOutcome::Resync`], get a new snapshot the way
/// [`snapshot_source`](Self::snapshot_source) says. Call
/// [`reset`](Self::reset) whenever the WebSocket reconnects.
pub trait VenueSync {
    /// The venue whose rules are applied.
    fn venue(&self) -> Venue;

    /// Process one raw frame. Frames that are valid JSON but carry nothing
    /// for this sync return [`FeedOutcome::Ignored`]; malformed book frames
    /// return an error and leave the sync unchanged.
    fn on_frame(&mut self, book: &mut OrderBook, frame: &str) -> Result<FeedOutcome>;

    /// True while the book is being kept current.
    fn is_live(&self) -> bool;

    /// True while the book is unusable until a snapshot arrives.
    fn needs_snapshot(&self) -> bool;

    /// Running counts.
    fn stats(&self) -> SyncStats;

    /// Forget all state (call it after a reconnect).
    fn reset(&mut self, book: &mut OrderBook);

    /// Where the next snapshot comes from.
    fn snapshot_source(&self) -> SnapshotSource {
        self.venue().snapshot_source()
    }
}

#[derive(Deserialize)]
struct BinanceProbe {
    #[serde(rename = "e", default)]
    event: Option<String>,
    #[serde(rename = "lastUpdateId", default)]
    last_update_id: Option<u64>,
}

/// Binance through the venue-neutral API: `on_frame` takes both diff-depth
/// WebSocket frames and REST depth snapshot bodies (told apart by their
/// `lastUpdateId` field). Before passing a snapshot body you can still
/// check [`BookSync::snapshot_is_too_old`]; if you do not, a too-old
/// snapshot comes back as a [`FeedOutcome::Resync`].
impl VenueSync for BookSync {
    fn venue(&self) -> Venue {
        Venue::Binance(self.market())
    }

    fn on_frame(&mut self, book: &mut OrderBook, frame: &str) -> Result<FeedOutcome> {
        let probe: BinanceProbe = serde_json::from_str(frame)?;
        if probe.last_update_id.is_some() {
            let snapshot = parse_depth_snapshot(frame)?;
            let done = self.on_snapshot(book, snapshot);
            return Ok(match done.resync {
                Some(reason) => FeedOutcome::Resync(GapReason::Binance(reason)),
                None if self.is_live() => FeedOutcome::Snapshot,
                // Nothing buffered connected yet: the snapshot is in, the
                // next event decides.
                None => FeedOutcome::Buffered,
            });
        }
        if probe.event.as_deref() != Some("depthUpdate") {
            return Ok(FeedOutcome::Ignored);
        }
        let update = parse_depth_update(frame)?;
        Ok(self.on_event(book, update).into())
    }

    fn is_live(&self) -> bool {
        BookSync::is_live(self)
    }

    fn needs_snapshot(&self) -> bool {
        BookSync::needs_snapshot(self)
    }

    fn stats(&self) -> SyncStats {
        BookSync::stats(self)
    }

    fn reset(&mut self, book: &mut OrderBook) {
        BookSync::reset(self, book)
    }
}

// ── Shared helpers for the stream-snapshot venues ────────────────────────────

/// Where a stream-snapshot sync (Coinbase, Kraken) is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// No snapshot yet (or the last one was dropped): updates are skipped.
    AwaitingSnapshot,
    /// A snapshot is applied and every message since has checked out.
    Live,
}

/// Fold the outcomes of several events in one frame into one: a resync
/// beats a snapshot, which beats an applied update, and so on.
pub(crate) fn merge(a: FeedOutcome, b: FeedOutcome) -> FeedOutcome {
    fn rank(o: &FeedOutcome) -> u8 {
        match o {
            FeedOutcome::Ignored => 0,
            FeedOutcome::Skipped => 1,
            FeedOutcome::Buffered => 2,
            FeedOutcome::Applied => 3,
            FeedOutcome::Snapshot => 4,
            FeedOutcome::Resync(_) => 5,
        }
    }
    if rank(&b) >= rank(&a) {
        b
    } else {
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderbook::book::Side;
    use rust_decimal_macros::dec;

    #[test]
    fn venue_names_round_trip() {
        for v in [
            Venue::Binance(Market::Spot),
            Venue::Binance(Market::UsdM),
            Venue::Coinbase,
            Venue::Kraken,
        ] {
            assert_eq!(Venue::parse(v.as_str()), Some(v));
        }
        assert_eq!(Venue::parse("BINANCE"), Some(Venue::Binance(Market::Spot)));
        assert_eq!(Venue::parse("bitstamp"), None);
        assert_eq!(Venue::Kraken.snapshot_source(), SnapshotSource::Stream);
        assert_eq!(
            Venue::Binance(Market::UsdM).snapshot_source(),
            SnapshotSource::Rest
        );
        assert_eq!(
            Venue::Kraken.default_ws_url("BTC/USD"),
            "wss://ws.kraken.com/v2"
        );
    }

    #[test]
    fn book_trait_derived_values() {
        let mut book = OrderBook::new();
        book.set_level(Side::Bid, dec!(100), dec!(3));
        book.set_level(Side::Ask, dec!(102), dec!(1));
        let b: &dyn Book = &book;
        assert_eq!(b.spread(), Some(dec!(2)));
        assert_eq!(b.mid_price(), Some(dec!(101)));
        assert_eq!(b.micro_price(), Some(dec!(101.5)));
        assert_eq!(b.depth_imbalance(5), Some(dec!(0.5)));
        assert!(!b.is_crossed());
        book.set_level(Side::Bid, dec!(102), dec!(1));
        assert!(Book::is_crossed(&book));
        assert_eq!(Book::mid_price(&OrderBook::new()), None);
    }

    #[test]
    fn binance_through_the_venue_api() {
        let mut book = OrderBook::new();
        let mut sync = BookSync::new(Market::Spot);
        let ev =
            r#"{"e":"depthUpdate","E":1,"s":"BTCUSDT","U":99,"u":102,"b":[["100.5","2"]],"a":[]}"#;
        assert_eq!(sync.on_frame(&mut book, ev).unwrap(), FeedOutcome::Buffered);
        assert!(VenueSync::needs_snapshot(&sync));
        assert_eq!(sync.snapshot_source(), SnapshotSource::Rest);
        let snap = r#"{"lastUpdateId":100,"bids":[["100.0","1"]],"asks":[["101.0","1"]]}"#;
        assert_eq!(
            sync.on_frame(&mut book, snap).unwrap(),
            FeedOutcome::Snapshot
        );
        assert_eq!(Book::best_bid(&book).unwrap().price, dec!(100.5));
        assert_eq!(
            sync.on_frame(&mut book, r#"{"result":null,"id":1}"#)
                .unwrap(),
            FeedOutcome::Ignored
        );
        let late = r#"{"e":"depthUpdate","E":2,"s":"BTCUSDT","U":110,"u":111,"b":[],"a":[]}"#;
        assert!(matches!(
            sync.on_frame(&mut book, late).unwrap(),
            FeedOutcome::Resync(GapReason::Binance(ResyncReason::Gap {
                expected: 103,
                got: 110
            }))
        ));
        assert!(sync.on_frame(&mut book, "not json").is_err());
        assert_eq!(sync.venue(), Venue::Binance(Market::Spot));
    }

    #[test]
    fn binance_snapshot_before_any_event_waits_for_the_stream() {
        let mut book = OrderBook::new();
        let mut sync = BookSync::new(Market::Spot);
        let snap = r#"{"lastUpdateId":100,"bids":[["100.0","1"]],"asks":[]}"#;
        assert_eq!(
            sync.on_frame(&mut book, snap).unwrap(),
            FeedOutcome::Buffered
        );
        let ev = r#"{"e":"depthUpdate","E":1,"s":"BTCUSDT","U":101,"u":101,"b":[],"a":[]}"#;
        assert_eq!(sync.on_frame(&mut book, ev).unwrap(), FeedOutcome::Applied);
    }

    #[test]
    fn merge_prefers_the_most_important_outcome() {
        let gap = FeedOutcome::Resync(GapReason::Sequence {
            expected: 1,
            got: 3,
        });
        assert_eq!(merge(FeedOutcome::Applied, gap), gap);
        assert_eq!(merge(gap, FeedOutcome::Applied), gap);
        assert_eq!(
            merge(FeedOutcome::Ignored, FeedOutcome::Skipped),
            FeedOutcome::Skipped
        );
        assert_eq!(
            merge(FeedOutcome::Snapshot, FeedOutcome::Applied),
            FeedOutcome::Snapshot
        );
    }
}
