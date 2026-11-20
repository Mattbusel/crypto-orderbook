//! The bug this crate exists for, with no network and no async runtime:
//! a Binance spot book, one dropped diff, and the moment it is caught.
//!
//! ```text
//! cargo run --example catch_a_gap --no-default-features
//! ```
//!
//! A book that applies every diff it receives would carry on after the
//! dropped event with the 0.5 BTC bid that was cancelled in it. `BookSync`
//! refuses: the next event does not start where the last one ended, so it
//! clears the book and asks for a new snapshot.

use crypto_orderbook::{
    parse_depth_snapshot, parse_depth_update, BookSync, Market, OrderBook, Outcome,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut book = OrderBook::new();
    let mut sync = BookSync::new(Market::Spot);

    // Binance's order: open the stream first, buffer, then fetch the snapshot.
    let first = r#"{"e":"depthUpdate","E":1,"s":"BTCUSDT","U":101,"u":102,"b":[["64250.10","0.50"]],"a":[]}"#;
    assert_eq!(
        sync.on_event(&mut book, parse_depth_update(first)?),
        Outcome::Buffered
    );

    let snapshot =
        r#"{"lastUpdateId":100,"bids":[["64250.00","1.20"]],"asks":[["64251.00","0.80"]]}"#;
    let done = sync.on_snapshot(&mut book, parse_depth_snapshot(snapshot)?);
    println!(
        "snapshot 100 applied, {} buffered event(s) replayed",
        done.applied
    );
    println!(
        "best bid {:?}",
        book.best_bid().map(|l| (l.price, l.quantity))
    );

    // Event 103..=104 cancels the 0.5 BTC bid. It never arrives.
    let _dropped =
        r#"{"e":"depthUpdate","E":2,"s":"BTCUSDT","U":103,"u":104,"b":[["64250.10","0"]],"a":[]}"#;

    // The next event starts at 105, not 103.
    let next = r#"{"e":"depthUpdate","E":3,"s":"BTCUSDT","U":105,"u":105,"b":[],"a":[["64251.00","0.70"]]}"#;
    match sync.on_event(&mut book, parse_depth_update(next)?) {
        Outcome::Resync(reason) => {
            println!("caught: {reason:?}");
            println!(
                "book cleared ({} bids), new snapshot needed: {}",
                book.bid_depth(),
                sync.needs_snapshot()
            );
        }
        other => return Err(format!("a gap went through as {other:?}").into()),
    }
    Ok(())
}
