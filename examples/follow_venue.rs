//! Follow a Coinbase or Kraken order book through the venue-neutral API:
//! one loop, any venue. Frames go into `VenueSync::on_frame`; on a resync
//! the example unsubscribes and subscribes again, which makes the venue send
//! a fresh snapshot. Kraken's checksum precision is read from its
//! `instrument` channel first.
//!
//! ```text
//! VENUE=kraken   SYMBOL=BTC/USD  cargo run --example follow_venue
//! VENUE=coinbase SYMBOL=BTC-USD  MESSAGES=50 cargo run --example follow_venue
//! RECORD=frames.ndjson ...       # also write every frame received, one per line
//! ```

use std::io::Write;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crypto_orderbook::venue::{
    parse_kraken_instruments, Book, CoinbaseSync, FeedOutcome, KrakenSync, Venue, VenueSync,
};
use crypto_orderbook::OrderBook;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let venue = std::env::var("VENUE")
        .ok()
        .and_then(|v| Venue::parse(&v))
        .unwrap_or(Venue::Kraken);
    let wanted: usize = std::env::var("MESSAGES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let mut record = match std::env::var("RECORD") {
        Ok(path) => Some(std::fs::File::create(path)?),
        Err(_) => None,
    };

    let (mut ws, _) = connect_async(venue.default_ws_url("")).await?;

    let (mut sync, subscribe, unsubscribe): (Box<dyn VenueSync>, String, String) = match venue {
        Venue::Coinbase => {
            let symbol = std::env::var("SYMBOL").unwrap_or_else(|_| "BTC-USD".into());
            let s = CoinbaseSync::new(symbol);
            let (sub, unsub) = (s.subscribe_message(), s.unsubscribe_message());
            (Box::new(s), sub, unsub)
        }
        Venue::Kraken => {
            let symbol = std::env::var("SYMBOL").unwrap_or_else(|_| "BTC/USD".into());
            let depth: usize = std::env::var("DEPTH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10);
            // The checksum needs the pair's precision: ask the instrument channel.
            ws.send(Message::Text(
                r#"{"method":"subscribe","params":{"channel":"instrument","snapshot":true}}"#
                    .into(),
            ))
            .await?;
            let precision = loop {
                let Some(frame) = ws.next().await else {
                    return Err("closed before instrument snapshot".into());
                };
                let Message::Text(text) = frame? else {
                    continue;
                };
                if let Some((_, p)) = parse_kraken_instruments(&text)?
                    .into_iter()
                    .find(|(s, _)| *s == symbol)
                {
                    break p;
                }
            };
            ws.send(Message::Text(
                r#"{"method":"unsubscribe","params":{"channel":"instrument"}}"#.into(),
            ))
            .await?;
            eprintln!(
                "{symbol}: price precision {}, qty precision {}",
                precision.price, precision.qty
            );
            let s = KrakenSync::new(symbol, depth, precision.price, precision.qty);
            let (sub, unsub) = (s.subscribe_message(), s.unsubscribe_message());
            (Box::new(s), sub, unsub)
        }
        other => {
            return Err(format!("{other} uses REST snapshots; see examples/follow_book.rs").into())
        }
    };

    ws.send(Message::Text(subscribe.clone())).await?;
    let mut book = OrderBook::new();
    let mut applied = 0;

    while let Some(frame) = ws.next().await {
        let Message::Text(text) = frame? else {
            continue;
        };
        if let Some(f) = record.as_mut() {
            writeln!(f, "{}", text.as_str())?;
        }
        match sync.on_frame(&mut book, &text)? {
            FeedOutcome::Snapshot => eprintln!(
                "snapshot: {} bids, {} asks",
                book.bid_depth(),
                book.ask_depth()
            ),
            FeedOutcome::Applied => {
                applied += 1;
                let (bid, ask) = (
                    Book::best_bid(&book).unwrap(),
                    Book::best_ask(&book).unwrap(),
                );
                println!(
                    "{:>4}  bid {} x {}  ask {} x {}  mid {}",
                    applied,
                    bid.price,
                    bid.quantity,
                    ask.price,
                    ask.quantity,
                    book.mid_price().unwrap()
                );
                if applied >= wanted {
                    break;
                }
            }
            FeedOutcome::Resync(reason) => {
                eprintln!("resync: {reason:?}; resubscribing");
                ws.send(Message::Text(unsubscribe.clone())).await?;
                ws.send(Message::Text(subscribe.clone())).await?;
            }
            _ => {}
        }
    }
    eprintln!("{:?}", sync.stats());
    Ok(())
}
