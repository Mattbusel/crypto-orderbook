//! Prometheus metrics, registered in the default registry.
//!
//! The service serves them at `/metrics`. A program that embeds the library
//! without the full HTTP API can mount [`router`] on its own server, or call
//! [`gather_text`] and expose the text however it likes.
//!
//! Sync health at a glance: `orderbook_synced` is 1 while the book is live,
//! `orderbook_resyncs_total` counts every time it had to be dropped (with
//! `orderbook_sequence_gaps_total` the subset caused by missed events), and
//! `orderbook_last_update_id` should keep rising.

use once_cell::sync::Lazy;
use prometheus::{register_counter, register_gauge, Counter, Gauge};

pub static SYNCED: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!(
        "orderbook_synced",
        "1 while the book is live and in sequence, 0 while syncing"
    )
    .unwrap()
});

pub static LAST_UPDATE_ID: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!(
        "orderbook_last_update_id",
        "Binance update id the book is correct as of (0 while syncing)"
    )
    .unwrap()
});

pub static BUFFERED_EVENTS: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!(
        "orderbook_buffered_events",
        "Depth events held while waiting for a snapshot"
    )
    .unwrap()
});

pub static RESYNCS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_resyncs_total",
        "Times the book was dropped and rebuilt: sequence gaps plus snapshots that did not connect"
    )
    .unwrap()
});

pub static EVENTS_SKIPPED_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_events_skipped_total",
        "Depth events dropped as already contained in the snapshot or already applied"
    )
    .unwrap()
});

pub static PARSE_ERRORS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_parse_errors_total",
        "Depth frames that could not be parsed"
    )
    .unwrap()
});

pub static SNAPSHOT_REFETCHES_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_snapshot_refetches_total",
        "Snapshots fetched again because they were older than the first buffered event"
    )
    .unwrap()
});

pub static SNAPSHOT_FAILURES_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_snapshot_failures_total",
        "Snapshot requests that failed for a reason other than a rate limit"
    )
    .unwrap()
});

pub static WS_EVENTS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_ws_events_total",
        "Depth events received and parsed from the Binance WebSocket"
    )
    .unwrap()
});

pub static WS_RECONNECTS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_ws_reconnects_total",
        "WebSocket reconnection count"
    )
    .unwrap()
});

pub static SNAPSHOTS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_snapshots_total",
        "REST snapshots successfully fetched"
    )
    .unwrap()
});

pub static SNAPSHOT_RATE_LIMITED_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_snapshot_rate_limited_total",
        "Snapshot requests answered 429 (rate limited) or 418 (IP banned) by Binance"
    )
    .unwrap()
});

pub static REST_USED_WEIGHT_1M: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!(
        "orderbook_rest_used_weight_1m",
        "Request weight used in the current minute, from Binance's X-MBX-USED-WEIGHT-1M header"
    )
    .unwrap()
});

pub static SEQUENCE_GAPS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_sequence_gaps_total",
        "Sequence gaps that triggered resync"
    )
    .unwrap()
});

pub static UPDATES_APPLIED_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!(
        "orderbook_updates_applied_total",
        "Depth updates applied to book"
    )
    .unwrap()
});

pub static BID_DEPTH: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!("orderbook_bid_depth", "Current bid-side price level count").unwrap()
});

pub static ASK_DEPTH: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!("orderbook_ask_depth", "Current ask-side price level count").unwrap()
});

pub static SPREAD: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!("orderbook_spread", "Current best spread in price units").unwrap()
});

pub static TRADE_VWAP_1M: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!(
        "orderbook_vwap_1m",
        "1-minute rolling VWAP from trade stream"
    )
    .unwrap()
});

pub static TRADE_VOLUME_1M: Lazy<Gauge> =
    Lazy::new(|| register_gauge!("orderbook_volume_1m", "1-minute rolling trade volume").unwrap());

/// Force all lazy statics to initialize so they appear in /metrics at zero
/// before any events arrive. This prevents missing metrics confusing dashboards.
pub fn init_all() {
    let _ = &*SYNCED;
    let _ = &*LAST_UPDATE_ID;
    let _ = &*BUFFERED_EVENTS;
    let _ = &*RESYNCS_TOTAL;
    let _ = &*EVENTS_SKIPPED_TOTAL;
    let _ = &*PARSE_ERRORS_TOTAL;
    let _ = &*SNAPSHOT_REFETCHES_TOTAL;
    let _ = &*SNAPSHOT_FAILURES_TOTAL;
    let _ = &*SNAPSHOT_RATE_LIMITED_TOTAL;
    let _ = &*REST_USED_WEIGHT_1M;
    let _ = &*WS_EVENTS_TOTAL;
    let _ = &*WS_RECONNECTS_TOTAL;
    let _ = &*SNAPSHOTS_TOTAL;
    let _ = &*SEQUENCE_GAPS_TOTAL;
    let _ = &*UPDATES_APPLIED_TOTAL;
    let _ = &*BID_DEPTH;
    let _ = &*ASK_DEPTH;
    let _ = &*SPREAD;
    let _ = &*TRADE_VWAP_1M;
    let _ = &*TRADE_VOLUME_1M;
}

/// Encode all registered metrics to Prometheus text exposition format.
pub fn gather_text() -> String {
    use prometheus::TextEncoder;
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    encoder
        .encode_to_string(&metric_families)
        .unwrap_or_default()
}

/// The Prometheus content type for [`gather_text`].
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// An axum router serving only `GET /metrics`, for programs that embed the
/// library and want the metrics endpoint without the rest of the API.
/// Calls [`init_all`] so every metric is listed from the start.
///
/// ```no_run
/// # async fn serve() {
/// let listener = tokio::net::TcpListener::bind("127.0.0.1:9100").await.unwrap();
/// axum::serve(listener, crypto_orderbook::metrics::router()).await.unwrap();
/// # }
/// ```
pub fn router() -> axum::Router {
    init_all();
    axum::Router::new().route(
        "/metrics",
        axum::routing::get(|| async {
            (
                [(axum::http::header::CONTENT_TYPE, CONTENT_TYPE)],
                gather_text(),
            )
        }),
    )
}
