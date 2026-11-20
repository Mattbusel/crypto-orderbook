// The service binary: a thin consumer of the crypto_orderbook library.

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};
use tracing::info;

use crypto_orderbook::api::routes::{router, AppState};
use crypto_orderbook::config::Config;
use crypto_orderbook::metrics;
use crypto_orderbook::orderbook::{self, book::OrderBook, manager::Manager};
use crypto_orderbook::trades::{manager::TradeManager, vwap::VwapWindow};
use crypto_orderbook::ws::client;

// How many messages the broadcast channel buffers per subscriber.
// If a slow client falls more than this many updates behind, it starts
// receiving Lagged errors and skips to the current state automatically.
const PUSH_CHANNEL_CAPACITY: usize = 64;

const HELP: &str = "crypto-orderbook: live Binance spot or USD-M futures order book served over HTTP, WebSocket and Prometheus

Usage: crypto-orderbook [--help | --version]
       crypto-orderbook replay <recording.ndjson>

Run it with no arguments to start the service. Settings come from environment variables:
  SYMBOL          trading pair to track            (default BTCUSDT)
  MARKET          spot or usdm (USD-M futures)      (default spot)
  DEPTH_LIMIT     snapshot levels; spot 5000 costs  (default 5000 spot, 1000 usdm)
                  250 request weight, 1000 costs 50
  API_PORT        HTTP port                         (default 3000)
  WS_BASE         depth diff WebSocket URL          (default Binance <symbol>@depth)
  TRADE_WS_BASE   trade WebSocket URL for VWAP      (default Binance <symbol>@trade)
  REST_BASE       REST base URL for the snapshot    (default per market)
  CHANNEL_BUFFER  events buffered during snapshot   (default 10000)
  RECORD_FILE     append every snapshot and raw depth event to this file
                  (NDJSON); rebuild it later with: crypto-orderbook replay FILE
  RUST_LOG        log filter, e.g. info

binance.com answers 451 from some regions (the US included). Use Binance.US there:
  REST_BASE=https://api.binance.us
  WS_BASE=wss://stream.binance.us:9443/ws/btcusdt@depth
  TRADE_WS_BASE=wss://stream.binance.us:9443/ws/btcusdt@trade

Endpoints once running (default port 3000):
  /health  /book/best  /book/snapshot?depth=N  /book/bids  /book/asks
  /book/spread  /book/midprice  /book/imbalance?depth=N  /book/vwap
  /metrics  /ws/book (WebSocket push)

Press Ctrl+C to stop.
";

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("crypto-orderbook {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.first().map(String::as_str) == Some("replay") {
        let Some(path) = args.get(1) else {
            eprintln!("usage: crypto-orderbook replay <recording.ndjson>");
            std::process::exit(2);
        };
        match orderbook::replay::replay(std::path::Path::new(path)) {
            Ok(r) => {
                println!(
                    "snapshots {}  events {}  applied {}  skipped {}  gaps {}  bad lines {}",
                    r.snapshots, r.events, r.applied, r.skipped, r.gaps, r.bad_lines
                );
                match (r.best_bid, r.best_ask) {
                    (Some((bp, bq)), Some((ap, aq))) => println!(
                        "final book at update {}: best bid {bp} x {bq}, best ask {ap} x {aq}, spread {}, {} bid / {} ask levels",
                        r.last_update_id.unwrap_or(0), ap - bp, r.bid_depth, r.ask_depth
                    ),
                    _ => println!("final book: not synced at the end of the recording"),
                }
                return;
            }
            Err(e) => {
                eprintln!("cannot replay {path}: {e}");
                std::process::exit(1);
            }
        }
    }
    if let Some(unknown) = args.first() {
        eprintln!(
            "unknown argument: {unknown}

{HELP}"
        );
        std::process::exit(2);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
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
        {
            let manager = Manager::new(depth_rx, Arc::clone(&book), cfg.symbol.clone(), push_tx.clone())
                .with_market(cfg.market, cfg.rest_base.clone(), cfg.depth_limit)
                .with_max_buffer(cfg.channel_buffer);
            match std::env::var_os("RECORD_FILE") {
                Some(path) => match orderbook::replay::Recorder::open(std::path::Path::new(&path)) {
                    Ok(rec) => {
                        info!(path = %std::path::Path::new(&path).display(), "recording the feed");
                        manager.with_recorder(rec)
                    }
                    Err(e) => {
                        eprintln!("cannot open RECORD_FILE: {e}");
                        std::process::exit(2);
                    }
                },
                None => manager,
            }
        }
        .run()
    );

    // Trade stream
    let (trade_tx, trade_rx) = mpsc::channel(cfg.channel_buffer);
    client::spawn(cfg.trade_ws_url(), trade_tx);
    tokio::spawn(TradeManager::new(trade_rx, Arc::clone(&vwap)).run());

    let state = AppState {
        book: Arc::clone(&book),
        vwap: Arc::clone(&vwap),
        symbol: cfg.symbol.clone(),
        push_tx: push_tx.clone(),
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.api_port));
    info!(%addr, "HTTP API listening");
    info!(
        "try http://localhost:{}/book/best once /health reports synced",
        cfg.api_port
    );

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
