/// Integration tests for the HTTP API layer.
///
/// These tests spin up a real Axum server on an ephemeral port, insert data
/// into the shared book directly (bypassing the WebSocket pipeline), and then
/// make real HTTP requests to verify the API behaves correctly end-to-end.
///
/// This tests things unit tests cannot: middleware stack, JSON serialisation,
/// routing, status codes, and the interaction between AppState clones.

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use rust_decimal_macros::dec;
use tokio::sync::broadcast;

use crypto_orderbook::api::routes::{AppState, router};
use crypto_orderbook::orderbook::book::OrderBook;
use crypto_orderbook::trades::vwap::VwapWindow;
use crypto_orderbook::ws::binance::{DepthUpdate, PriceLevel};

// ── Test helpers ───────────────────────────────────────────────────────────────

/// Spawn a server on a random available port. Returns the base URL.
async fn spawn_server(book: Arc<RwLock<OrderBook>>) -> String {
    let vwap    = Arc::new(RwLock::new(VwapWindow::new(Duration::from_secs(60))));
    let (tx, _) = broadcast::channel(16);

    let state = AppState {
        book,
        vwap,
        symbol:  "TESTUSDT".into(),
        push_tx: tx,
    };

    // Port 0 asks the OS to assign a free ephemeral port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failed");
    let addr: SocketAddr = listener.local_addr().expect("no local addr");

    tokio::spawn(async move {
        axum::serve(listener, router(state)).await.unwrap();
    });

    format!("http://{addr}")
}

fn make_update(first: u64, last: u64, bids: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)>, asks: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)>) -> DepthUpdate {
    DepthUpdate {
        first_update_id: first,
        last_update_id:  last,
        bids: bids.into_iter().map(|(p, q)| PriceLevel { price: p, quantity: q }).collect(),
        asks: asks.into_iter().map(|(p, q)| PriceLevel { price: p, quantity: q }).collect(),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn health_returns_503_when_book_empty() {
    let book   = Arc::new(RwLock::new(OrderBook::new()));
    let base   = spawn_server(Arc::clone(&book)).await;
    let client = reqwest::Client::new();

    let resp = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), 503);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "syncing");
    assert_eq!(body["synced"], false);
}

#[tokio::test]
async fn health_returns_200_once_book_has_data() {
    let book = Arc::new(RwLock::new(OrderBook::new()));
    book.write().unwrap().apply(&make_update(1, 5,
        vec![(dec!(50000), dec!(1))],
        vec![(dec!(50100), dec!(1))],
    ));

    let base   = spawn_server(Arc::clone(&book)).await;
    let client = reqwest::Client::new();

    let resp = client.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["synced"], true);
    assert_eq!(body["bid_depth"], 1);
    assert_eq!(body["ask_depth"], 1);
}

#[tokio::test]
async fn best_returns_correct_bid_ask_and_spread() {
    let book = Arc::new(RwLock::new(OrderBook::new()));
    book.write().unwrap().apply(&make_update(1, 1,
        vec![(dec!(50000), dec!(2)), (dec!(49999), dec!(1))],
        vec![(dec!(50100), dec!(0.5))],
    ));

    let base   = spawn_server(Arc::clone(&book)).await;
    let client = reqwest::Client::new();
    let body: serde_json::Value = client
        .get(format!("{base}/book/best"))
        .send().await.unwrap()
        .json().await.unwrap();

    assert_eq!(body["best_bid"]["price"], "50000");
    assert_eq!(body["best_ask"]["price"], "50100");
    assert_eq!(body["spread"], "100");
    assert_eq!(body["symbol"], "TESTUSDT");
}

#[tokio::test]
async fn snapshot_respects_depth_param() {
    let book = Arc::new(RwLock::new(OrderBook::new()));
    let bids: Vec<_> = (0..10).map(|i| (dec!(50000) - rust_decimal::Decimal::from(i), dec!(1))).collect();
    book.write().unwrap().apply(&make_update(1, 1, bids, vec![]));

    let base   = spawn_server(Arc::clone(&book)).await;
    let client = reqwest::Client::new();

    // Default depth is 20 — with only 10 bids, should return 10.
    let body: serde_json::Value = client
        .get(format!("{base}/book/snapshot"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(body["bids"].as_array().unwrap().len(), 10);

    // depth=3 should return only the top 3.
    let body3: serde_json::Value = client
        .get(format!("{base}/book/snapshot?depth=3"))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(body3["bids"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn snapshot_caps_depth_at_100() {
    let book = Arc::new(RwLock::new(OrderBook::new()));
    // Insert 150 bid levels.
    let bids: Vec<_> = (0..150u32)
        .map(|i| (dec!(50000) - rust_decimal::Decimal::from(i), dec!(1)))
        .collect();
    book.write().unwrap().apply(&make_update(1, 1, bids, vec![]));

    let base   = spawn_server(Arc::clone(&book)).await;
    let client = reqwest::Client::new();

    let body: serde_json::Value = client
        .get(format!("{base}/book/snapshot?depth=200"))
        .send().await.unwrap()
        .json().await.unwrap();

    // Even though 150 levels exist and 200 was requested, cap is 100.
    assert_eq!(body["bids"].as_array().unwrap().len(), 100);
}

#[tokio::test]
async fn midprice_is_between_bid_and_ask() {
    let book = Arc::new(RwLock::new(OrderBook::new()));
    // Equal quantities -> micro_price should equal simple midpoint.
    book.write().unwrap().apply(&make_update(1, 1,
        vec![(dec!(50000), dec!(1))],
        vec![(dec!(50100), dec!(1))],
    ));

    let base   = spawn_server(Arc::clone(&book)).await;
    let client = reqwest::Client::new();
    let body: serde_json::Value = client
        .get(format!("{base}/book/midprice"))
        .send().await.unwrap()
        .json().await.unwrap();

    // With equal quantities, micro_price == arithmetic mid == 50050.
    assert_eq!(body.as_str().unwrap(), "50050");
}

#[tokio::test]
async fn metrics_endpoint_returns_prometheus_text() {
    // init_all() is normally called from main(). Force it here so metrics
    // are registered before the endpoint is hit.
    crypto_orderbook::metrics::init_all();

    let book = Arc::new(RwLock::new(OrderBook::new()));
    let base  = spawn_server(Arc::clone(&book)).await;
    let client = reqwest::Client::new();

    let resp = client.get(format!("{base}/metrics")).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let ct = resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.contains("text/plain"), "content-type was: {ct}");

    let body = resp.text().await.unwrap();
    // Prometheus format: lines starting with # HELP and # TYPE
    assert!(body.contains("# HELP"), "no HELP comment in metrics output");
}

#[tokio::test]
async fn imbalance_detects_buy_pressure() {
    let book = Arc::new(RwLock::new(OrderBook::new()));
    // Heavy bid side: 10 BTC bid, 1 BTC ask -> imbalance = (10-1)/(10+1) = 0.818...
    book.write().unwrap().apply(&make_update(1, 1,
        vec![(dec!(50000), dec!(10))],
        vec![(dec!(50100), dec!(1))],
    ));

    let base   = spawn_server(Arc::clone(&book)).await;
    let client = reqwest::Client::new();
    let body: serde_json::Value = client
        .get(format!("{base}/book/imbalance"))
        .send().await.unwrap()
        .json().await.unwrap();

    assert_eq!(body["interpretation"], "strong buy pressure");
    // imbalance should be positive (bid-heavy)
    let imb: f64 = body["imbalance"].as_str().unwrap().parse().unwrap();
    assert!(imb > 0.5, "expected imbalance > 0.5, got {imb}");
}
