mod api;
mod config;
mod error;
mod orderbook;
mod ws;

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc;
use tracing::info;

use api::routes::{router, AppState};
use config::Config;
use orderbook::{book::OrderBook, manager::Manager};
use ws::client;

#[tokio::main]
async fn main() {
    // Structured logging. RUST_LOG=info shows normal operation.
    // RUST_LOG=debug shows every event processed.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cfg = Config::from_env();
    info!(symbol = %cfg.symbol, port = cfg.api_port, "crypto-orderbook starting");

    // The order book lives behind an Arc so it can be shared between
    // the manager (writer) and the API layer (readers) without copying.
    let book = Arc::new(RwLock::new(OrderBook::new()));

    // The channel that carries WebSocket events from the client task
    // to the manager task. The buffer absorbs events that arrive while
    // the manager is busy fetching a REST snapshot.
    let (tx, rx) = mpsc::channel(cfg.channel_buffer);

    // Spawn the WebSocket client. It runs forever in the background,
    // reconnecting as needed and forwarding events into the channel.
    client::spawn(cfg.ws_url(), tx);

    // Spawn the manager. It owns the channel receiver and drives the
    // sync state machine, writing to the book when updates are valid.
    let manager = Manager::new(rx, Arc::clone(&book), cfg.symbol.clone());
    tokio::spawn(manager.run());

    // Start the HTTP API. This task runs in the foreground — when it
    // exits (which it never should), main() returns and the process ends.
    let state = AppState {
        book: Arc::clone(&book),
        symbol: cfg.symbol.clone(),
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.api_port));
    info!(%addr, "HTTP API listening");

    let listener = tokio::net::TcpListener::bind(addr).await
        .expect("failed to bind API port");

    axum::serve(listener, router(state))
        .await
        .expect("HTTP server error");
}
