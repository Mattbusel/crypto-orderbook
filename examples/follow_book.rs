//! Keep a Binance spot order book in sync with your own I/O, using only the
//! library's sans-IO `BookSync`: open the diff-depth stream, fetch a REST
//! snapshot once an event is buffered, refetch while the snapshot is too
//! old, then apply events and resync on a gap.
//!
//! ```text
//! cargo run --example follow_book                       # binance.com, 30 events
//! REST_BASE=https://api.binance.us \
//! WS_URL=wss://stream.binance.us:9443/ws/btcusdt@depth \
//! EVENTS=10 cargo run --example follow_book             # Binance.US
//! ```

use futures_util::StreamExt;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crypto_orderbook::{parse_depth_snapshot, parse_depth_update, BookSync, Market, OrderBook, Outcome};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let symbol = std::env::var("SYMBOL").unwrap_or_else(|_| "BTCUSDT".into());
    let rest = std::env::var("REST_BASE").unwrap_or_else(|_| Market::Spot.default_rest_base().into());
    let ws = std::env::var("WS_URL").unwrap_or_else(|_| Market::Spot.default_ws_url(&symbol));
    let wanted: usize = std::env::var("EVENTS").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    let snapshot_url = format!("{rest}{}?symbol={symbol}&limit=100", Market::Spot.depth_path());

    let (mut stream, _) = connect_async(ws.as_str()).await?;
    let http = reqwest::Client::new();
    let mut book = OrderBook::new();
    let mut sync = BookSync::new(Market::Spot);
    let mut applied = 0;

    while let Some(frame) = stream.next().await {
        let Message::Text(text) = frame? else { continue };
        let Ok(update) = parse_depth_update(&text) else { continue };

        match sync.on_event(&mut book, update) {
            Outcome::Applied => {
                applied += 1;
                let (bid, ask) = (book.best_bid().unwrap(), book.best_ask().unwrap());
                println!(
                    "update {:>12}  bid {} x {}  ask {} x {}",
                    book.last_update_id, bid.price, bid.quantity, ask.price, ask.quantity
                );
                if applied >= wanted {
                    break;
                }
            }
            Outcome::Resync(reason) => eprintln!("resync: {reason:?}"),
            Outcome::Buffered | Outcome::Skipped => {}
        }

        // Binance's order: stream first, then the snapshot, refetched while
        // it is older than the first buffered event. (A real program would
        // fetch concurrently with reading the stream; the socket buffers
        // events meanwhile, which is enough here.)
        if sync.needs_snapshot() {
            loop {
                let body = http.get(&snapshot_url).send().await?.error_for_status()?.text().await?;
                let snapshot = parse_depth_snapshot(&body)?;
                if sync.snapshot_is_too_old(snapshot.last_update_id) {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    continue;
                }
                let done = sync.on_snapshot(&mut book, snapshot);
                eprintln!(
                    "snapshot applied: {} buffered events applied, {} already in it, resync {:?}",
                    done.applied, done.skipped, done.resync
                );
                break;
            }
        }
    }
    eprintln!("{:?}", sync.stats());
    Ok(())
}
