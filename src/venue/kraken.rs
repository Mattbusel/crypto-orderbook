//! Kraken spot WebSocket v2 `book` channel as a sans-IO state machine, with
//! CRC32 checksum validation after every message.
//!
//! The rules ([book channel](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/book),
//! [checksum guide](https://docs.kraken.com/exchange/guides/websockets/book-checksum-v2)):
//!
//! - Subscribe on `wss://ws.kraken.com/v2` with
//!   `{"method":"subscribe","params":{"channel":"book","symbol":[..],"depth":N}}`
//!   (`N` is 10, 25, 100, 500 or 1000). The first message is a `snapshot` of
//!   the top `N` levels; later ones are `update`s.
//! - Levels carry absolute quantities; `qty` 0 removes the level. Apply every
//!   level of a message in order, then **truncate the book to `N`**: Kraken
//!   does not send removals for levels pushed out of the subscribed depth.
//! - Every message carries `checksum`, the CRC32 of the top 10 asks (low to
//!   high) then the top 10 bids (high to low), each level written as price
//!   then quantity with the decimal point and leading zeros removed. Prices
//!   and quantities are written with the pair's `price_precision` and
//!   `qty_precision` (from the `instrument` channel; see
//!   [`parse_kraken_instruments`]). Kraken's own channel example only
//!   checks out with them (`0.566` must be written `0.5660`).
//!
//! The stream has no sequence numbers: the checksum is how a lost or
//! misapplied message shows. [`KrakenSync`] checks it after every message
//! and on a mismatch drops the book ([`GapReason::Checksum`]); unsubscribe
//! and subscribe again for a fresh snapshot.

// The crate-wide `AppError` is large (it can hold a WebSocket error); the
// parsers return it like the Binance ones do.
#![allow(clippy::result_large_err)]

use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{merge, Book, FeedOutcome, GapReason, StreamState, Venue, VenueSync};
use crate::error::{AppError, Result};
use crate::orderbook::book::{OrderBook, Side};
use crate::orderbook::sync::SyncStats;

/// Kraken spot WebSocket v2 public endpoint.
pub const WS_URL: &str = "wss://ws.kraken.com/v2";

/// Number of levels per side covered by the checksum.
pub const CHECKSUM_LEVELS: usize = 10;

// ── CRC32 (IEEE 802.3, the zlib one) ─────────────────────────────────────────

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for &b in bytes {
        crc = CRC_TABLE[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

// ── Checksum ─────────────────────────────────────────────────────────────────

/// Decimal places Kraken writes a pair's prices and quantities with, from
/// the `instrument` channel (`price_precision`, `qty_precision`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KrakenPrecision {
    /// Decimal places of prices.
    pub price: u32,
    /// Decimal places of quantities.
    pub qty: u32,
}

impl KrakenPrecision {
    /// `price` and `qty` decimal places.
    pub fn new(price: u32, qty: u32) -> Self {
        Self { price, qty }
    }
}

fn checksum_digits(value: Decimal, places: Option<u32>, out: &mut String) {
    let mut v = value;
    if let Some(dp) = places {
        v.rescale(dp);
    }
    let s = v.to_string();
    let start = out.len();
    out.extend(s.chars().filter(|&c| c != '.'));
    let zeros = out[start..].bytes().take_while(|&b| b == b'0').count();
    out.replace_range(start..start + zeros, "");
}

/// The input string Kraken's checksum is the CRC32 of (asks low to high,
/// then bids high to low, top 10 each). With `precision` `None`, each
/// value is written with the scale it was parsed with, which matches
/// Kraken only when it sent every digit (as in its checksum guide).
pub fn kraken_checksum_input(book: &impl Book, precision: Option<KrakenPrecision>) -> String {
    let (pp, qp) = (precision.map(|p| p.price), precision.map(|p| p.qty));
    let mut s = String::with_capacity(512);
    for level in book
        .top_asks(CHECKSUM_LEVELS)
        .into_iter()
        .chain(book.top_bids(CHECKSUM_LEVELS))
    {
        checksum_digits(level.price, pp, &mut s);
        checksum_digits(level.quantity, qp, &mut s);
    }
    s
}

/// Kraken's v2 book checksum of `book`: CRC32 of
/// [`kraken_checksum_input`].
///
/// ```
/// use crypto_orderbook::venue::{kraken_checksum, KrakenPrecision};
/// use crypto_orderbook::{OrderBook, Side};
///
/// let mut book = OrderBook::new();
/// book.set_level(Side::Ask, "45285.2".parse().unwrap(), "0.001".parse().unwrap());
/// book.set_level(Side::Bid, "45283.5".parse().unwrap(), "0.1".parse().unwrap());
/// // With BTC/USD's precision (1, 8) the input is "452852" "100000" "452835" "10000000".
/// let crc = kraken_checksum(&book, Some(KrakenPrecision::new(1, 8)));
/// // As received ("0.001", "0.1") the digits differ, and so does the checksum.
/// assert_ne!(crc, kraken_checksum(&book, None));
/// ```
pub fn kraken_checksum(book: &impl Book, precision: Option<KrakenPrecision>) -> u32 {
    !crc32_update(!0, kraken_checksum_input(book, precision).as_bytes())
}

// ── Wire format ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RawEnvelope {
    #[serde(default)]
    channel: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    data: Option<Box<RawValue>>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct RawBookData {
    symbol: String,
    #[serde(default)]
    bids: Vec<RawLevel>,
    #[serde(default)]
    asks: Vec<RawLevel>,
    checksum: u32,
    #[serde(default)]
    timestamp: Option<String>,
}

#[derive(Deserialize)]
struct RawLevel {
    price: Box<RawValue>,
    qty: Box<RawValue>,
}

/// Read a JSON number (or numeric string) without going through `f64`, so
/// `0.10000000` keeps its eight places.
fn exact(field: &str, raw: &RawValue) -> Result<Decimal> {
    let text = raw.get().trim();
    let text = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    text.parse::<Decimal>()
        .or_else(|_| Decimal::from_scientific(text))
        .map_err(|e| AppError::InvalidField(format!("invalid {field} '{text}': {e}")))
}

fn levels(raw: &[RawLevel]) -> Result<Vec<(Decimal, Decimal)>> {
    raw.iter()
        .map(|l| Ok((exact("price", &l.price)?, exact("qty", &l.qty)?)))
        .collect()
}

// ── Domain types ─────────────────────────────────────────────────────────────

/// One symbol's part of a `book` message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KrakenBookData {
    /// The pair, for example `BTC/USD`.
    pub symbol: String,
    /// `(price, qty)` bid levels in the order sent; `qty` 0 removes.
    pub bids: Vec<(Decimal, Decimal)>,
    /// `(price, qty)` ask levels in the order sent.
    pub asks: Vec<(Decimal, Decimal)>,
    /// Kraken's CRC32 of its top 10 levels after this message.
    pub checksum: u32,
    /// The message timestamp, if present.
    pub timestamp: Option<String>,
}

/// One frame from the Kraken v2 WebSocket, reduced to what book sync needs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KrakenMessage {
    /// A `book` channel snapshot (`snapshot` true) or update.
    Book {
        /// True for `"type":"snapshot"`.
        snapshot: bool,
        /// One entry per symbol.
        data: Vec<KrakenBookData>,
    },
    /// Anything else: heartbeats, status, other channels, method responses.
    Other {
        /// The `channel`, if any.
        channel: Option<String>,
        /// The `method` of a response (`subscribe`, `unsubscribe`, ...).
        method: Option<String>,
        /// The `error` of a failed request.
        error: Option<String>,
    },
}

/// Parse one Kraken v2 WebSocket frame. Every valid JSON object parses;
/// a malformed `book` frame is an error.
pub fn parse_kraken_message(raw: &str) -> Result<KrakenMessage> {
    let env: RawEnvelope = serde_json::from_str(raw)?;
    if env.channel.as_deref() != Some("book") || env.method.is_some() {
        return Ok(KrakenMessage::Other {
            channel: env.channel,
            method: env.method,
            error: env.error,
        });
    }
    let snapshot = match env.kind.as_deref() {
        Some("snapshot") => true,
        Some("update") => false,
        other => {
            return Err(AppError::InvalidField(format!(
                "invalid book message type {other:?}"
            )))
        }
    };
    let raw_data: Vec<RawBookData> = match env.data {
        Some(d) => serde_json::from_str(d.get())?,
        None => Vec::new(),
    };
    let data = raw_data
        .into_iter()
        .map(|d| {
            Ok(KrakenBookData {
                bids: levels(&d.bids)?,
                asks: levels(&d.asks)?,
                symbol: d.symbol,
                checksum: d.checksum,
                timestamp: d.timestamp,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(KrakenMessage::Book { snapshot, data })
}

#[derive(Deserialize)]
struct RawInstrumentData {
    #[serde(default)]
    pairs: Vec<RawPair>,
}

#[derive(Deserialize)]
struct RawPair {
    symbol: String,
    price_precision: u32,
    qty_precision: u32,
}

/// Read each pair's checksum precision from an `instrument` channel
/// message (subscribe with `{"method":"subscribe","params":{"channel":"instrument"}}`).
/// Returns an empty list for any other message.
pub fn parse_kraken_instruments(raw: &str) -> Result<Vec<(String, KrakenPrecision)>> {
    let env: RawEnvelope = serde_json::from_str(raw)?;
    if env.channel.as_deref() != Some("instrument") {
        return Ok(Vec::new());
    }
    let Some(data) = env.data else {
        return Ok(Vec::new());
    };
    let data: RawInstrumentData = serde_json::from_str(data.get())?;
    Ok(data
        .pairs
        .into_iter()
        .map(|p| {
            (
                p.symbol,
                KrakenPrecision::new(p.price_precision, p.qty_precision),
            )
        })
        .collect())
}

// ── KrakenSync ───────────────────────────────────────────────────────────────

/// Kraken's v2 `book` for one symbol, as a sans-IO state machine that
/// validates the checksum after every snapshot and update.
///
/// ```
/// use crypto_orderbook::venue::{FeedOutcome, GapReason, KrakenSync, VenueSync};
/// use crypto_orderbook::OrderBook;
///
/// let mut book = OrderBook::new();
/// let mut sync = KrakenSync::new("MATIC/USD", 10, 4, 8);
/// // A snapshot whose checksum does not match its levels.
/// let snap = r#"{"channel":"book","type":"snapshot","data":[{"symbol":"MATIC/USD",
///   "bids":[{"price":0.5666,"qty":4831.75496356}],"asks":[{"price":0.5668,"qty":4410.79769741}],
///   "checksum":0}]}"#;
/// // A wrong checksum is caught at once.
/// assert!(matches!(
///     sync.on_frame(&mut book, snap).unwrap(),
///     FeedOutcome::Resync(GapReason::Checksum { expected: 0, .. })
/// ));
/// assert!(sync.needs_snapshot());
/// ```
#[derive(Debug, Clone)]
pub struct KrakenSync {
    symbol: String,
    depth: usize,
    precision: Option<KrakenPrecision>,
    state: StreamState,
    stats: SyncStats,
    last_checksum: Option<u32>,
}

impl KrakenSync {
    /// A sync for `symbol` (for example `BTC/USD`) subscribed at `depth`
    /// levels, whose prices and quantities have `price_precision` and
    /// `qty_precision` decimal places (the pair's `price_precision` and
    /// `qty_precision` in the `instrument` channel).
    pub fn new(
        symbol: impl Into<String>,
        depth: usize,
        price_precision: u32,
        qty_precision: u32,
    ) -> Self {
        Self::build(
            symbol.into(),
            depth,
            Some(KrakenPrecision::new(price_precision, qty_precision)),
        )
    }

    /// Like [`new`](Self::new), but writes each value with the decimal
    /// places it arrived with instead of the pair's precision. Use it only
    /// for feeds that send every digit (such as numeric strings).
    pub fn with_received_precision(symbol: impl Into<String>, depth: usize) -> Self {
        Self::build(symbol.into(), depth, None)
    }

    fn build(symbol: String, depth: usize, precision: Option<KrakenPrecision>) -> Self {
        Self {
            symbol,
            depth: depth.max(CHECKSUM_LEVELS),
            precision,
            state: StreamState::AwaitingSnapshot,
            stats: SyncStats::default(),
            last_checksum: None,
        }
    }

    /// The symbol this sync follows.
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// The subscribed depth the book is truncated to.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The precision used for checksums (`None`: as received).
    pub fn precision(&self) -> Option<KrakenPrecision> {
        self.precision
    }

    /// The current state.
    pub fn state(&self) -> StreamState {
        self.state
    }

    /// The last checksum that matched.
    pub fn last_checksum(&self) -> Option<u32> {
        self.last_checksum
    }

    /// The subscribe request for this symbol's book at this depth.
    pub fn subscribe_message(&self) -> String {
        serde_json::json!({"method": "subscribe", "params": {"channel": "book", "symbol": [self.symbol], "depth": self.depth}})
            .to_string()
    }

    /// The matching unsubscribe request. Send it and then
    /// [`subscribe_message`](Self::subscribe_message) for a fresh snapshot
    /// after a resync.
    pub fn unsubscribe_message(&self) -> String {
        serde_json::json!({"method": "unsubscribe", "params": {"channel": "book", "symbol": [self.symbol], "depth": self.depth}})
            .to_string()
    }

    /// Process one parsed message.
    pub fn on_message(&mut self, book: &mut OrderBook, msg: &KrakenMessage) -> FeedOutcome {
        let KrakenMessage::Book { snapshot, data } = msg else {
            return FeedOutcome::Ignored;
        };
        let mut out = FeedOutcome::Ignored;
        for entry in data {
            if entry.symbol != self.symbol {
                continue;
            }
            self.stats.events += 1;
            let step = if *snapshot {
                book.clear();
                self.apply(book, entry);
                self.verify(book, entry.checksum, FeedOutcome::Snapshot)
            } else if self.state == StreamState::Live {
                self.apply(book, entry);
                self.verify(book, entry.checksum, FeedOutcome::Applied)
            } else {
                self.stats.skipped += 1;
                FeedOutcome::Skipped
            };
            out = merge(out, step);
        }
        out
    }

    fn apply(&self, book: &mut OrderBook, entry: &KrakenBookData) {
        for &(price, qty) in &entry.bids {
            book.set_level(Side::Bid, price, qty);
        }
        for &(price, qty) in &entry.asks {
            book.set_level(Side::Ask, price, qty);
        }
        book.truncate(self.depth);
    }

    fn verify(&mut self, book: &mut OrderBook, expected: u32, ok: FeedOutcome) -> FeedOutcome {
        let computed = kraken_checksum(&*book, self.precision);
        if computed == expected {
            if ok == FeedOutcome::Snapshot {
                self.stats.snapshots += 1;
            } else {
                self.stats.applied += 1;
            }
            self.state = StreamState::Live;
            self.last_checksum = Some(expected);
            ok
        } else {
            book.clear();
            self.state = StreamState::AwaitingSnapshot;
            self.last_checksum = None;
            self.stats.resyncs += 1;
            FeedOutcome::Resync(GapReason::Checksum { expected, computed })
        }
    }
}

impl VenueSync for KrakenSync {
    fn venue(&self) -> Venue {
        Venue::Kraken
    }

    fn on_frame(&mut self, book: &mut OrderBook, frame: &str) -> Result<FeedOutcome> {
        let msg = parse_kraken_message(frame)?;
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

    fn reset(&mut self, book: &mut OrderBook) {
        book.clear();
        self.state = StreamState::AwaitingSnapshot;
        self.last_checksum = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(!crc32_update(!0, b"123456789"), 0xCBF4_3926);
        assert_eq!(!crc32_update(!0, b""), 0);
    }

    #[test]
    fn checksum_digits_follow_the_guide() {
        let mut s = String::new();
        checksum_digits(dec!(45285.2), None, &mut s);
        checksum_digits(dec!(0.00100000), None, &mut s);
        assert_eq!(s, "452852100000");
        let mut s = String::new();
        checksum_digits(dec!(0.566), Some(4), &mut s);
        checksum_digits(dec!(18097.1547), Some(8), &mut s);
        assert_eq!(s, "56601809715470000");
    }

    #[test]
    fn numbers_keep_their_written_precision() {
        let m = parse_kraken_message(
            r#"{"channel":"book","type":"update","data":[{"symbol":"BTC/USD","bids":[{"price":45283.5,"qty":0.10000000}],"asks":[{"price":"1e2","qty":"1"}],"checksum":7}]}"#,
        )
        .unwrap();
        let KrakenMessage::Book { snapshot, data } = m else {
            panic!()
        };
        assert!(!snapshot);
        assert_eq!(data[0].bids[0].1.to_string(), "0.10000000");
        assert_eq!(data[0].asks[0].0, dec!(100));
        assert_eq!(data[0].checksum, 7);
    }

    #[test]
    fn other_messages_are_not_book_data() {
        for frame in [
            r#"{"channel":"heartbeat"}"#,
            r#"{"channel":"status","type":"update","data":[{"system":"online","api_version":"v2"}]}"#,
            r#"{"method":"subscribe","result":{"channel":"book","depth":10,"snapshot":true,"symbol":"ALGO/USD"},"success":true}"#,
        ] {
            assert!(matches!(
                parse_kraken_message(frame).unwrap(),
                KrakenMessage::Other { .. }
            ));
        }
        assert!(parse_kraken_message(r#"{"channel":"book","type":"update","data":[{"symbol":"X","bids":[{"price":"x","qty":1}],"asks":[],"checksum":1}]}"#).is_err());
        assert!(parse_kraken_message(r#"{"channel":"book","type":"weird","data":[]}"#).is_err());
    }

    #[test]
    fn instrument_precision_is_read() {
        let raw = r#"{"channel":"instrument","type":"snapshot","data":{"assets":[],"pairs":[
            {"symbol":"BTC/USD","base":"BTC","quote":"USD","status":"online","qty_precision":8,"qty_increment":1e-08,"price_precision":1}]}}"#;
        assert_eq!(
            parse_kraken_instruments(raw).unwrap(),
            vec![("BTC/USD".to_string(), KrakenPrecision::new(1, 8))]
        );
        assert!(parse_kraken_instruments(r#"{"channel":"heartbeat"}"#)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn depth_is_at_least_the_checksum_depth() {
        assert_eq!(KrakenSync::new("BTC/USD", 1, 1, 8).depth(), 10);
        assert_eq!(KrakenSync::new("BTC/USD", 25, 1, 8).depth(), 25);
        let v: serde_json::Value =
            serde_json::from_str(&KrakenSync::new("BTC/USD", 25, 1, 8).subscribe_message())
                .unwrap();
        assert_eq!(v["params"]["depth"], 25);
        assert_eq!(v["params"]["symbol"][0], "BTC/USD");
    }
}
