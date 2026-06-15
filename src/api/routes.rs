use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use tower_http::timeout::TimeoutLayer;

use crate::api::ws_push::{ws_book_handler, PushSender};
use crate::orderbook::book::OrderBook;
use crate::trades::vwap::VwapWindow;

// ── Shared state ───────────────────────────────────────────────────────────────

/// Everything the API layer needs. Axum clones this into every handler call.
/// Cloning an Arc is O(1) — it just increments a reference count.
/// The order book and VWAP window are never copied.
#[derive(Clone)]
pub struct AppState {
    pub book:    Arc<RwLock<OrderBook>>,
    pub vwap:    Arc<RwLock<VwapWindow>>,
    pub symbol:  String,
    /// Broadcast channel for pushing top-of-book snapshots to WebSocket clients.
    /// Cloning a Sender is O(1) and safe across tasks.
    pub push_tx: PushSender,
}

// ── Response shapes ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PriceLevelResponse {
    pub price:    Decimal,
    pub quantity: Decimal,
}

#[derive(Serialize)]
pub struct BestResponse {
    pub symbol:        String,
    pub best_bid:      Option<PriceLevelResponse>,
    pub best_ask:      Option<PriceLevelResponse>,
    pub spread:        Option<Decimal>,
    pub micro_price:   Option<Decimal>,
    pub last_update_id: u64,
}

#[derive(Serialize)]
pub struct SnapshotResponse {
    pub symbol:         String,
    pub last_update_id: u64,
    pub bids:           Vec<PriceLevelResponse>,
    pub asks:           Vec<PriceLevelResponse>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status:         &'static str,
    pub synced:         bool,
    pub bid_depth:      usize,
    pub ask_depth:      usize,
    pub last_update_id: u64,
}

#[derive(Serialize)]
pub struct ImbalanceResponse {
    pub symbol:         String,
    pub depth:          usize,
    pub imbalance:      Option<Decimal>,
    pub interpretation: &'static str,
}

#[derive(Serialize)]
pub struct VwapResponse {
    pub symbol:    String,
    pub vwap_1m:   Option<Decimal>,
    pub volume_1m: Decimal,
}

#[derive(Deserialize)]
pub struct DepthParams {
    #[serde(default = "default_depth")]
    pub depth: usize,
}

fn default_depth() -> usize { 20 }

// ── Router ─────────────────────────────────────────────────────────────────────

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health",           get(health))
        .route("/book/best",        get(best))
        .route("/book/snapshot",    get(snapshot))
        .route("/book/bids",        get(bids))
        .route("/book/asks",        get(asks))
        .route("/book/spread",      get(spread))
        .route("/book/midprice",    get(midprice))
        .route("/book/imbalance",   get(imbalance))
        .route("/book/vwap",        get(vwap_handler))
        .route("/metrics",          get(metrics_handler))
        .route("/ws/book",          get(ws_book_handler))
        // Kill requests that stall for more than 10 seconds.
        // Without this, a lock-contended request holds a thread indefinitely.
        .layer(TimeoutLayer::new(Duration::from_secs(10)))
        .with_state(state)
}

// ── Handlers ───────────────────────────────────────────────────────────────────

async fn health(State(s): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let book = s.book.read().expect("book lock poisoned");
    let synced = book.last_update_id > 0;
    let code = if synced { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (code, Json(HealthResponse {
        status:         if synced { "ok" } else { "syncing" },
        synced,
        bid_depth:      book.bid_depth(),
        ask_depth:      book.ask_depth(),
        last_update_id: book.last_update_id,
    }))
}

async fn best(State(s): State<AppState>) -> Json<BestResponse> {
    let book = s.book.read().expect("book lock poisoned");
    Json(BestResponse {
        symbol:         s.symbol.clone(),
        best_bid:       book.best_bid().map(level_to_response),
        best_ask:       book.best_ask().map(level_to_response),
        spread:         book.spread(),
        micro_price:    book.micro_price(),
        last_update_id: book.last_update_id,
    })
}

async fn snapshot(
    State(s): State<AppState>,
    Query(p): Query<DepthParams>,
) -> Json<SnapshotResponse> {
    let depth = p.depth.min(100);
    let book  = s.book.read().expect("book lock poisoned");
    Json(SnapshotResponse {
        symbol:         s.symbol.clone(),
        last_update_id: book.last_update_id,
        bids:           book.top_bids(depth).into_iter().map(level_to_response).collect(),
        asks:           book.top_asks(depth).into_iter().map(level_to_response).collect(),
    })
}

async fn bids(State(s): State<AppState>, Query(p): Query<DepthParams>) -> Json<Vec<PriceLevelResponse>> {
    let book = s.book.read().expect("book lock poisoned");
    Json(book.top_bids(p.depth.min(100)).into_iter().map(level_to_response).collect())
}

async fn asks(State(s): State<AppState>, Query(p): Query<DepthParams>) -> Json<Vec<PriceLevelResponse>> {
    let book = s.book.read().expect("book lock poisoned");
    Json(book.top_asks(p.depth.min(100)).into_iter().map(level_to_response).collect())
}

async fn spread(State(s): State<AppState>) -> Json<Option<Decimal>> {
    Json(s.book.read().expect("book lock poisoned").spread())
}

async fn midprice(State(s): State<AppState>) -> Json<Option<Decimal>> {
    Json(s.book.read().expect("book lock poisoned").micro_price())
}

async fn imbalance(
    State(s): State<AppState>,
    Query(p): Query<DepthParams>,
) -> Json<ImbalanceResponse> {
    let depth = p.depth.min(100);
    let book  = s.book.read().expect("book lock poisoned");
    let imb   = book.depth_imbalance(depth);
    let interpretation = match imb.and_then(|v| v.to_f64()) {
        None        => "insufficient data",
        Some(v) if v >  0.3 => "strong buy pressure",
        Some(v) if v >  0.1 => "mild buy pressure",
        Some(v) if v < -0.3 => "strong sell pressure",
        Some(v) if v < -0.1 => "mild sell pressure",
        _                   => "balanced",
    };
    Json(ImbalanceResponse { symbol: s.symbol.clone(), depth, imbalance: imb, interpretation })
}

async fn vwap_handler(State(s): State<AppState>) -> Json<VwapResponse> {
    let window = s.vwap.read().expect("vwap lock poisoned");
    Json(VwapResponse {
        symbol:    s.symbol.clone(),
        vwap_1m:   window.vwap(),
        volume_1m: window.volume(),
    })
}

/// Prometheus text exposition. Content-Type is the Prometheus standard.
async fn metrics_handler() -> Response {
    let body = crate::metrics::gather_text();
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
        .into_response()
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn level_to_response(l: crate::orderbook::book::PriceLevel) -> PriceLevelResponse {
    PriceLevelResponse { price: l.price, quantity: l.quantity }
}
