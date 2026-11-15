//! Replay tests for the Coinbase and Kraken syncs.
//!
//! Two kinds of input:
//!
//! - **The venues' own documentation samples**, copied into fixtures as
//!   published (whitespace removed; Coinbase's examples also had trailing
//!   commas, which are not valid JSON):
//!   - `kraken_v2_book_docs.ndjson`: the subscribe responses, snapshot and
//!     update from Kraken's WebSocket v2 `book` channel reference.
//!   - `kraken_v2_checksum_guide.json`: the BTC/USD snapshot from Kraken's
//!     v2 book checksum guide, whose intermediate string and CRC32 the guide
//!     spells out.
//!   - `coinbase_l2_docs.ndjson`: the `level2` snapshot and `heartbeats`
//!     message examples from Coinbase's Advanced Trade WebSocket channel docs.
//! - **Live recordings** made with `examples/follow_venue.rs` on 2026-11-15:
//!   - `kraken_btcusd_d10_live.ndjson`: BTC/USD at depth 10, 40 updates.
//!   - `kraken_ethusd_d25_live.ndjson`: ETH/USD at depth 25, 300 updates.
//!   - `coinbase_btcusd_live.ndjson`: BTC-USD `level2`, 100 update frames
//!     plus the `subscriptions` ack. The snapshot is cut to its best 100
//!     levels per side to keep the file small; everything else is verbatim.

use crypto_orderbook::orderbook::book::PriceLevel;
use crypto_orderbook::venue::{
    kraken_checksum, kraken_checksum_input, Book, CoinbaseSync, FeedOutcome, GapReason,
    KrakenPrecision, KrakenSync, VenueSync,
};
use crypto_orderbook::OrderBook;
use rust_decimal::Decimal;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn lines(name: &str) -> Vec<String> {
    fixture(name)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn top(book: &OrderBook, n: usize) -> (Vec<PriceLevel>, Vec<PriceLevel>) {
    (Book::top_bids(book, n), Book::top_asks(book, n))
}

fn replay(sync: &mut dyn VenueSync, book: &mut OrderBook, frames: &[String]) -> Vec<FeedOutcome> {
    frames
        .iter()
        .map(|f| sync.on_frame(book, f).unwrap())
        .collect()
}

// ── Kraken: documentation samples ────────────────────────────────────────────

#[test]
fn kraken_book_channel_example_validates_with_the_pairs_precision() {
    let frames = lines("kraken_v2_book_docs.ndjson");
    let mut book = OrderBook::new();
    // MATIC/USD: price precision 4, qty precision 8.
    let mut sync = KrakenSync::new("MATIC/USD", 10, 4, 8);
    let outcomes = replay(&mut sync, &mut book, &frames);
    assert_eq!(
        outcomes,
        vec![
            FeedOutcome::Ignored,
            FeedOutcome::Ignored,
            FeedOutcome::Snapshot,
            FeedOutcome::Applied
        ]
    );
    assert_eq!(sync.last_checksum(), Some(2114181697));
    assert_eq!(book.best_bid().unwrap().price, d("0.5666"));
    assert_eq!(book.best_ask().unwrap().price, d("0.5668"));
    let tenth = Book::top_bids(&book, 10)[9];
    assert_eq!(
        (tenth.price, tenth.quantity),
        (d("0.5657"), d("1098.3947558"))
    );
    assert_eq!(sync.stats().resyncs, 0);

    // The same frames mean nothing to an ALGO/USD book.
    let mut other = KrakenSync::new("ALGO/USD", 10, 4, 8);
    let mut other_book = OrderBook::new();
    assert!(replay(&mut other, &mut other_book, &frames)
        .iter()
        .all(|o| *o == FeedOutcome::Ignored));
}

#[test]
fn kraken_book_channel_example_fails_without_the_pairs_precision() {
    // The example writes 0.566 and 18097.1547; the checksum was computed
    // over 0.5660 and 18097.15470000. Without the pair's precision the
    // snapshot is rejected rather than trusted.
    let frames = lines("kraken_v2_book_docs.ndjson");
    let mut book = OrderBook::new();
    let mut sync = KrakenSync::with_received_precision("MATIC/USD", 10);
    let outcomes = replay(&mut sync, &mut book, &frames[..3]);
    assert!(matches!(
        outcomes[2],
        FeedOutcome::Resync(GapReason::Checksum {
            expected: 2439117997,
            ..
        })
    ));
    assert!(sync.needs_snapshot());
    assert_eq!(book.bid_depth(), 0);
}

#[test]
fn kraken_checksum_guide_example_reproduces_the_string_and_the_crc() {
    let frame = fixture("kraken_v2_checksum_guide.json");
    for mut sync in [
        KrakenSync::new("BTC/USD", 10, 1, 8),
        KrakenSync::with_received_precision("BTC/USD", 10),
    ] {
        let mut book = OrderBook::new();
        assert_eq!(
            sync.on_frame(&mut book, &frame).unwrap(),
            FeedOutcome::Snapshot
        );
        assert_eq!(
            kraken_checksum_input(&book, sync.precision()),
            "45285210000045286415457195345286615457110945289615456091145290215890660452918154553491\
             452947445474945296135380000452975994554245299518772827\
             452835100000004528341545820154528211000000045281010000000452803154592586452790799000045277633101034527753000000045277315460273745276615445238"
        );
        assert_eq!(kraken_checksum(&book, sync.precision()), 3310070434);
    }
}

#[test]
fn kraken_tampered_update_is_caught_and_a_new_snapshot_recovers() {
    let frames = lines("kraken_v2_book_docs.ndjson");
    let tampered = frames[3].replace("1098.3947558", "1098.3947559");
    let mut book = OrderBook::new();
    let mut sync = KrakenSync::new("MATIC/USD", 10, 4, 8);
    replay(&mut sync, &mut book, &frames[..3]);
    assert!(matches!(
        sync.on_frame(&mut book, &tampered).unwrap(),
        FeedOutcome::Resync(GapReason::Checksum {
            expected: 2114181697,
            ..
        })
    ));
    assert!(sync.needs_snapshot());
    assert_eq!(book.bid_depth() + book.ask_depth(), 0);
    // Updates are not applied until the resubscribe snapshot arrives.
    assert_eq!(
        sync.on_frame(&mut book, &frames[3]).unwrap(),
        FeedOutcome::Skipped
    );
    assert_eq!(
        sync.on_frame(&mut book, &frames[2]).unwrap(),
        FeedOutcome::Snapshot
    );
    assert_eq!(
        sync.on_frame(&mut book, &frames[3]).unwrap(),
        FeedOutcome::Applied
    );
    assert_eq!(sync.stats().resyncs, 1);
}

// ── Kraken: live recordings ──────────────────────────────────────────────────

fn kraken_live(name: &str, symbol: &str, depth: usize, precision: KrakenPrecision) {
    let frames = lines(name);
    let mut book = OrderBook::new();
    let mut sync = KrakenSync::new(symbol, depth, precision.price, precision.qty);
    let mut truth_top = Vec::new();
    for frame in &frames {
        let outcome = sync.on_frame(&mut book, frame).unwrap();
        assert!(!outcome.is_resync(), "{name}: {outcome:?} on {frame}");
        if matches!(outcome, FeedOutcome::Snapshot | FeedOutcome::Applied) {
            assert!(!book.is_crossed());
            assert!(book.bid_depth() <= depth && book.ask_depth() <= depth);
        }
        truth_top.push(top(&book, 10));
    }
    let stats = sync.stats();
    assert_eq!((stats.snapshots, stats.resyncs), (1, 0));
    assert!(stats.applied >= 40, "{stats:?}");

    // Drop each update in turn. Kraken has no sequence numbers, so the
    // checksum is the only guard: whenever the sync still applies a message,
    // its top 10 levels must be the true ones; otherwise it must resync.
    let updates: Vec<usize> = (0..frames.len())
        .filter(|&i| frames[i].contains(r#""type":"update""#))
        .collect();
    let mut caught = 0;
    for &dropped in &updates {
        let mut book = OrderBook::new();
        let mut sync = KrakenSync::new(symbol, depth, precision.price, precision.qty);
        let mut resynced = false;
        for (i, frame) in frames.iter().enumerate() {
            if i == dropped {
                continue;
            }
            match sync.on_frame(&mut book, frame).unwrap() {
                FeedOutcome::Applied => assert_eq!(
                    top(&book, 10),
                    truth_top[i],
                    "{name}: dropped {dropped}, frame {i}"
                ),
                FeedOutcome::Resync(GapReason::Checksum { .. }) => {
                    resynced = true;
                    break;
                }
                _ => {}
            }
        }
        caught += usize::from(resynced);
    }
    // Most single drops change the top 10 and are caught on the next update.
    assert!(
        caught * 2 > updates.len(),
        "{name}: only {caught} of {} drops caught",
        updates.len()
    );
}

#[test]
fn kraken_live_btcusd_depth_10_replays_with_every_checksum_matching() {
    kraken_live(
        "kraken_btcusd_d10_live.ndjson",
        "BTC/USD",
        10,
        KrakenPrecision::new(1, 8),
    );
}

#[test]
fn kraken_live_ethusd_depth_25_replays_with_truncation_and_every_checksum_matching() {
    kraken_live(
        "kraken_ethusd_d25_live.ndjson",
        "ETH/USD",
        25,
        KrakenPrecision::new(2, 8),
    );
}

// ── Coinbase: documentation samples ──────────────────────────────────────────

#[test]
fn coinbase_level2_and_heartbeat_examples() {
    let frames = lines("coinbase_l2_docs.ndjson");
    let mut book = OrderBook::new();
    let mut sync = CoinbaseSync::new("BTC-USD");
    assert_eq!(
        sync.on_frame(&mut book, &frames[0]).unwrap(),
        FeedOutcome::Snapshot
    );
    assert!(sync.is_live());
    assert_eq!(book.bid_depth(), 2);
    assert_eq!(book.best_bid().unwrap().price, d("21921.73"));
    assert_eq!(book.best_bid().unwrap().quantity, d("0.06317902"));
    assert_eq!(sync.last_sequence(), Some(0));

    // Both examples carry sequence_num 0. On one connection that cannot
    // happen twice: the second is a repeat and is skipped.
    assert_eq!(
        sync.on_frame(&mut book, &frames[1]).unwrap(),
        FeedOutcome::Skipped
    );

    // On its own connection the heartbeat is accepted and changes no book.
    let mut fresh = CoinbaseSync::new("BTC-USD");
    let mut empty = OrderBook::new();
    assert_eq!(
        fresh.on_frame(&mut empty, &frames[1]).unwrap(),
        FeedOutcome::Ignored
    );
    assert_eq!(fresh.last_sequence(), Some(0));
    assert!(fresh.needs_snapshot());

    // An ETH-USD book ignores the BTC-USD snapshot.
    let mut eth = CoinbaseSync::new("ETH-USD");
    assert_eq!(
        eth.on_frame(&mut empty, &frames[0]).unwrap(),
        FeedOutcome::Ignored
    );
}

// ── Coinbase: live recording ─────────────────────────────────────────────────

#[test]
fn coinbase_live_btcusd_replays_in_sequence() {
    let frames = lines("coinbase_btcusd_live.ndjson");
    let mut book = OrderBook::new();
    let mut sync = CoinbaseSync::new("BTC-USD");
    let mut tops = Vec::new();
    for (i, frame) in frames.iter().enumerate() {
        let outcome = sync.on_frame(&mut book, frame).unwrap();
        assert!(
            matches!(
                outcome,
                FeedOutcome::Snapshot | FeedOutcome::Applied | FeedOutcome::Ignored
            ),
            "frame {i}: {outcome:?}"
        );
        assert_eq!(sync.last_sequence(), Some(i as u64));
        assert!(!book.is_crossed(), "frame {i}");
        tops.push(top(&book, 20));
    }
    let stats = sync.stats();
    assert_eq!((stats.snapshots, stats.resyncs, stats.skipped), (1, 0, 0));
    assert_eq!(stats.applied, frames.len() as u64 - 2); // minus the snapshot and the subscriptions ack

    // Re-delivering any frame changes nothing.
    let mut book2 = OrderBook::new();
    let mut sync2 = CoinbaseSync::new("BTC-USD");
    for (i, frame) in frames.iter().enumerate() {
        sync2.on_frame(&mut book2, frame).unwrap();
        assert_eq!(
            sync2.on_frame(&mut book2, frame).unwrap(),
            FeedOutcome::Skipped
        );
        assert_eq!(top(&book2, 20), tops[i]);
    }

    // Dropping any frame after the snapshot (book or not) is caught by the
    // very next frame, with the exact sequence numbers.
    for dropped in 1..frames.len() - 1 {
        let mut book = OrderBook::new();
        let mut sync = CoinbaseSync::new("BTC-USD");
        for (i, frame) in frames.iter().enumerate() {
            if i == dropped {
                continue;
            }
            let outcome = sync.on_frame(&mut book, frame).unwrap();
            if i == dropped + 1 {
                assert_eq!(
                    outcome,
                    FeedOutcome::Resync(GapReason::Sequence {
                        expected: dropped as u64,
                        got: i as u64
                    })
                );
                assert_eq!(book.bid_depth() + book.ask_depth(), 0);
            } else if i > dropped + 1 {
                assert!(!sync.is_live());
            } else {
                assert_eq!(top(&book, 20), tops[i]);
            }
        }
    }
}
