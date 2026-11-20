//! Correctly sequenced order books for Binance, Coinbase and Kraken, as a
//! library, and a Binance order book service.
//!
//! # Library
//!
//! The core is [`BookSync`], Binance's documented "how to manage a local
//! order book correctly" procedure as a sans-IO state machine: you bring the
//! WebSocket frames and REST snapshots (from any client, a recording, or a
//! test), it keeps an [`OrderBook`] correct or tells you it needs a new
//! snapshot. Spot and USD-M futures sequencing are both implemented; see
//! [`orderbook::sync`] for the exact rules.
//!
//! ```
//! use crypto_orderbook::{parse_depth_snapshot, parse_depth_update, BookSync, Market, OrderBook, Outcome};
//!
//! let mut book = OrderBook::new();
//! let mut sync = BookSync::new(Market::Spot);
//!
//! // 1. Open the stream first; events are buffered until a snapshot arrives.
//! let frame = r#"{"e":"depthUpdate","E":1,"s":"BTCUSDT","U":157,"u":160,"b":[["0.0024","10"]],"a":[]}"#;
//! assert_eq!(sync.on_event(&mut book, parse_depth_update(frame).unwrap()), Outcome::Buffered);
//!
//! // 2. Fetch GET /api/v3/depth once sync.needs_snapshot(); refetch while it is too old.
//! assert!(sync.needs_snapshot());
//! let body = r#"{"lastUpdateId":158,"bids":[["0.0024","14"]],"asks":[["0.0026","100"]]}"#;
//! let snapshot = parse_depth_snapshot(body).unwrap();
//! assert!(!sync.snapshot_is_too_old(snapshot.last_update_id));
//! sync.on_snapshot(&mut book, snapshot);
//!
//! // 3. Keep feeding events; on Outcome::Resync fetch a new snapshot.
//! assert!(sync.is_live());
//! assert_eq!(book.best_bid().unwrap().quantity.to_string(), "10");
//! ```
//!
//! # Other venues
//!
//! [`venue`] runs Coinbase Advanced Trade `level2` ([`CoinbaseSync`],
//! per-connection sequence numbers) and Kraken WebSocket v2 `book`
//! ([`KrakenSync`], CRC32 checksum after every message) on the same sans-IO
//! design, behind one API: the [`Book`] trait to read any venue's book and
//! the [`VenueSync`] trait to feed raw frames to any venue's sync
//! (implemented by [`BookSync`] too).
//!
//! Other building blocks:
//!
//! - [`orderbook::replay`]: record the raw feed to NDJSON and rebuild the
//!   book from it offline through the same [`BookSync`].
//! - [`orderbook::manager::Manager`] with [`ws::client::spawn`]: the
//!   networked driver the service uses (reconnects, rate-limit-aware
//!   snapshots, circuit breaker), writing to a shared `Arc<RwLock<OrderBook>>`.
//! - [`metrics`]: Prometheus metrics; [`metrics::router`] serves `/metrics`.
//! - [`api::routes::router`]: the full HTTP and WebSocket API.
//!
//! # Service
//!
//! The `crypto-orderbook` binary wires all of the above together; run
//! `crypto-orderbook --help` for its settings.

// The service's `AppError::WebSocket` wraps tungstenite's error, which is large; the
// `Result`s that carry it are on the reconnect path, not per message, so it stays unboxed.
#![cfg_attr(feature = "service", allow(clippy::result_large_err))]

#[cfg(feature = "service")]
pub mod api;
#[cfg(feature = "service")]
pub mod config;
pub mod error;
#[cfg(feature = "service")]
pub mod metrics;
pub mod orderbook;
pub mod trades;
pub mod venue;
pub mod ws;

pub use orderbook::book::{OrderBook, Side};
pub use orderbook::sync::{
    BookSync, Market, Outcome, ResyncReason, SnapshotOutcome, SyncState, SyncStats,
};
pub use venue::{
    Book, CoinbaseSync, FeedOutcome, GapReason, KrakenSync, SnapshotSource, Venue, VenueSync,
};
pub use ws::binance::{
    parse_depth_snapshot, parse_depth_update, DepthSnapshot, DepthUpdate, PriceLevel,
};
