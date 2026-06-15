mod api;
mod config;
mod error;
mod metrics;
mod orderbook;
mod trades;
mod ws;

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};
use tracing::info;

use api::routes::{router, AppState};
use config::Config;
use orderbook::{book::OrderBook, manager::Manager};
use trades::{manager::TradeManager, vwap::VwapWindow};
use ws::client;

// How many messages the broadcast channel buffers per subscriber.
// If a slow client falls more than this many updates behind, it starts
// receiving Lagged errors and skips to the current state automatically.
const PUSH_CHANNEL_CAPACITY: usize = 64;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cfg = Config::from_env();
    info!(symbol = %cfg.symbol, port = cfg.api_port, "crypto-orderbook starting");

    // Force all Prometheus metrics to zero before any events arrive.
    metrics::init_all();

    let book = Arc::new(RwLock::new(OrderBook::new()));
    let vwap = Arc::new(RwLock::new(VwapWindow::new(Duration::from_secs(60))));

    // Broadcast channel: Manager publishes here, every WS subscriber reads it.
    // broadcast::channel returns (Sender, Receiver). We keep the Sender and
    // distribute it. Each subscriber calls .subscribe() to get their own Receiver.
    let (push_tx, _initial_rx) = broadcast::channel(PUSH_CHANNEL_CAPACITY);

    // Depth stream
    let (depth_tx, depth_rx) = mpsc::channel(cfg.channel_buffer);
    client::spawn(cfg.ws_url(), depth_tx);
    tokio::spawn(
        Manager::new(depth_rx, Arc::clone(&book), cfg.symbol.clone(), push_tx.clone()).run()
    );

    // Trade stream
    let (trade_tx, trade_rx) = mpsc::channel(cfg.channel_buffer);
    client::spawn(cfg.trade_ws_url(), trade_tx);
    tokio::spawn(TradeManager::new(trade_rx, Arc::clone(&vwap)).run());

    let state = AppState {
        book:    Arc::clone(&book),
        vwap:    Arc::clone(&vwap),
        symbol:  cfg.symbol.clone(),
        push_tx: push_tx.clone(),
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.api_port));
    info!(%addr, "HTTP API listening");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind API port");

    tokio::select! {
        result = axum::serve(listener, router(state)) => {
            result.expect("HTTP server error");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("shutdown signal received, draining in-flight requests");
        }
    }
}
