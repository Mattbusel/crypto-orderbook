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

use tokio::sync::mpsc;
use tracing::info;

use api::routes::{router, AppState};
use config::Config;
use orderbook::{book::OrderBook, manager::Manager};
use trades::{manager::TradeManager, vwap::VwapWindow};
use ws::client;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cfg = Config::from_env();
    info!(symbol = %cfg.symbol, port = cfg.api_port, "crypto-orderbook starting");

    // Force all Prometheus metrics to register at zero before any events arrive.
    // Without this, counters that never fire are absent from /metrics entirely,
    // which confuses dashboards that expect them to always exist.
    metrics::init_all();

    // Shared order book — written by the Manager, read by every API handler.
    let book = Arc::new(RwLock::new(OrderBook::new()));

    // Shared VWAP window — written by the TradeManager, read by /book/vwap.
    let vwap = Arc::new(RwLock::new(VwapWindow::new(Duration::from_secs(60))));

    // Depth stream: channel between the WebSocket client and the book Manager.
    let (depth_tx, depth_rx) = mpsc::channel(cfg.channel_buffer);
    client::spawn(cfg.ws_url(), depth_tx);
    tokio::spawn(Manager::new(depth_rx, Arc::clone(&book), cfg.symbol.clone()).run());

    // Trade stream: separate WebSocket subscription for VWAP calculation.
    let (trade_tx, trade_rx) = mpsc::channel(cfg.channel_buffer);
    client::spawn(cfg.trade_ws_url(), trade_tx);
    tokio::spawn(TradeManager::new(trade_rx, Arc::clone(&vwap)).run());

    let state = AppState {
        book:   Arc::clone(&book),
        vwap:   Arc::clone(&vwap),
        symbol: cfg.symbol.clone(),
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.api_port));
    info!(%addr, "HTTP API listening");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind API port");

    // tokio::select! races the HTTP server against SIGTERM/Ctrl-C.
    // Whichever finishes first wins. In production, SIGTERM wins:
    // axum drains in-flight requests before the process exits.
    tokio::select! {
        result = axum::serve(listener, router(state)) => {
            result.expect("HTTP server error");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("shutdown signal received, draining in-flight requests");
        }
    }
}
