use once_cell::sync::Lazy;
use prometheus::{register_counter, register_gauge, Counter, Gauge};

pub static WS_EVENTS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!("orderbook_ws_events_total", "Depth events received from Binance WS").unwrap()
});

pub static WS_RECONNECTS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!("orderbook_ws_reconnects_total", "WebSocket reconnection count").unwrap()
});

pub static SNAPSHOTS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!("orderbook_snapshots_total", "REST snapshots successfully fetched").unwrap()
});

pub static SEQUENCE_GAPS_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!("orderbook_sequence_gaps_total", "Sequence gaps that triggered resync").unwrap()
});

pub static UPDATES_APPLIED_TOTAL: Lazy<Counter> = Lazy::new(|| {
    register_counter!("orderbook_updates_applied_total", "Depth updates applied to book").unwrap()
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
    register_gauge!("orderbook_vwap_1m", "1-minute rolling VWAP from trade stream").unwrap()
});

pub static TRADE_VOLUME_1M: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!("orderbook_volume_1m", "1-minute rolling trade volume").unwrap()
});

/// Force all lazy statics to initialize so they appear in /metrics at zero
/// before any events arrive. This prevents missing metrics confusing dashboards.
pub fn init_all() {
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
    use prometheus::{Encoder as _, TextEncoder};
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    encoder.encode_to_string(&metric_families).unwrap_or_default()
}
