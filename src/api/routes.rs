use std::sync::{Arc, RwLock};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::orderbook::book::OrderBook;

// ── Shared state ───────────────────────────────────────────────────────────────

/// The only thing the API layer knows about: a reference to the book.
/// Axum clones this into every request handler automatically.
#[derive(Clone)]
pub struct AppState {
    pub book: Arc<RwLock<OrderBook>>,
    pub symbol: String,
}

// ── Response shapes ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PriceLevelResponse {
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Serialize)]
pub struct BestResponse {
    pub symbol: String,
    pub best_bid: Option<PriceLevelResponse>,
    pub best_ask: Option<PriceLevelResponse>,
    pub spread: Option<Decimal>,
    pub last_update_id: u64,
}

#[derive(Serialize)]
pub struct SnapshotResponse {
    pub symbol: String,
    pub last_update_id: u64,
    pub bids: Vec<PriceLevelResponse>,
    pub asks: Vec<PriceLevelResponse>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub synced: bool,
    pub bid_depth: usize,
    pub ask_depth: usize,
    pub last_update_id: u64,
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
        .route("/health",         get(health))
        .route("/book/best",      get(best))
        .route("/book/snapshot",  get(snapshot))
        .route("/book/bids",      get(bids))
        .route("/book/asks",      get(asks))
        .route("/book/spread",    get(spread))
        .with_state(state)
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// Health check. Returns 200 if the book has been synced at least once,
/// 503 if we are still waiting for the first snapshot.
/// Downstream load balancers and monitoring use this to decide if
/// the instance is ready to serve traffic.
async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let book = state.book.read().expect("book lock poisoned");
    let synced = book.last_update_id > 0;
    let status = if synced { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(HealthResponse {
        status: if synced { "ok" } else { "syncing" },
        synced,
        bid_depth: book.bid_depth(),
        ask_depth: book.ask_depth(),
        last_update_id: book.last_update_id,
    }))
}

/// The single most important query: what is the best price to buy at
/// and the best price to sell at right now, and what is the gap between them.
async fn best(State(state): State<AppState>) -> Json<BestResponse> {
    let book = state.book.read().expect("book lock poisoned");
    Json(BestResponse {
        symbol: state.symbol.clone(),
        best_bid: book.best_bid().map(|l| PriceLevelResponse { price: l.price, quantity: l.quantity }),
        best_ask: book.best_ask().map(|l| PriceLevelResponse { price: l.price, quantity: l.quantity }),
        spread: book.spread(),
        last_update_id: book.last_update_id,
    })
}

/// The top N levels on both sides. Default 20. Cap at 100 to prevent
/// a caller from requesting the entire book and causing a slow response.
async fn snapshot(
    State(state): State<AppState>,
    Query(params): Query<DepthParams>,
) -> Json<SnapshotResponse> {
    let depth = params.depth.min(100);
    let book = state.book.read().expect("book lock poisoned");
    Json(SnapshotResponse {
        symbol: state.symbol.clone(),
        last_update_id: book.last_update_id,
        bids: book.top_bids(depth)
            .into_iter()
            .map(|l| PriceLevelResponse { price: l.price, quantity: l.quantity })
            .collect(),
        asks: book.top_asks(depth)
            .into_iter()
            .map(|l| PriceLevelResponse { price: l.price, quantity: l.quantity })
            .collect(),
    })
}

async fn bids(
    State(state): State<AppState>,
    Query(params): Query<DepthParams>,
) -> Json<Vec<PriceLevelResponse>> {
    let depth = params.depth.min(100);
    let book = state.book.read().expect("book lock poisoned");
    Json(book.top_bids(depth)
        .into_iter()
        .map(|l| PriceLevelResponse { price: l.price, quantity: l.quantity })
        .collect())
}

async fn asks(
    State(state): State<AppState>,
    Query(params): Query<DepthParams>,
) -> Json<Vec<PriceLevelResponse>> {
    let depth = params.depth.min(100);
    let book = state.book.read().expect("book lock poisoned");
    Json(book.top_asks(depth)
        .into_iter()
        .map(|l| PriceLevelResponse { price: l.price, quantity: l.quantity })
        .collect())
}

async fn spread(State(state): State<AppState>) -> Json<Option<Decimal>> {
    let book = state.book.read().expect("book lock poisoned");
    Json(book.spread())
}
