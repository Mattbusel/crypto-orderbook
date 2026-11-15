//! Replay tests on real recorded Binance messages.
//!
//! The fixtures are two recordings the service made itself (`RECORD_FILE`)
//! of Binance.US BTCUSDT spot `@depth` on 2026-11-15, from two instances
//! started about 12 seconds apart, each with its own WebSocket connection
//! and its own REST snapshot (limit 100). Both are cut at the same update
//! id, 3430341996. Recording A's snapshot (3430336954) lands in the middle
//! of an event (U 3430336671, u 3430336972), so the overlapping handshake
//! is exercised on real data.
//!
//! The strongest check here needs no expected values written by hand: two
//! books built from different snapshots and different connections must be
//! identical at every update id they share. Any off-by-one in the handshake
//! or continuity rules, or a wrongly applied level, makes them disagree.

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crypto_orderbook::orderbook::replay::{replay_reader, Record};
use crypto_orderbook::{parse_depth_update, BookSync, OrderBook, Outcome};

const A: &str = include_str!("fixtures/binance_us_btcusdt_a.ndjson");
const B: &str = include_str!("fixtures/binance_us_btcusdt_b.ndjson");
const LAST_ID: u64 = 3430341996;

/// Levels compared between the two books. The snapshots carry 100 levels a
/// side; levels deeper than that window are known only from diffs, so each
/// book can legitimately lack far levels the other has. The top 20 are well
/// inside both windows.
const TOP: usize = 20;

type Levels = Vec<(Decimal, Decimal)>;

/// Run a recording through `BookSync` and return the top of the book after
/// every applied event, keyed by update id.
fn books_by_update(recording: &str) -> BTreeMap<u64, (Levels, Levels)> {
    let mut book = OrderBook::new();
    let mut sync: Option<BookSync> = None;
    let mut early = Vec::new();
    let mut out = BTreeMap::new();
    for line in recording.lines() {
        let record: Record = serde_json::from_str(line).expect("fixture line parses");
        if let Some((market, snapshot)) = record.to_snapshot() {
            let s = sync.get_or_insert_with(|| BookSync::new(market));
            for update in early.drain(..) {
                s.on_event(&mut book, update);
            }
            assert_eq!(s.on_snapshot(&mut book, snapshot).resync, None);
            if s.is_live() {
                out.insert(book.last_update_id, top(&book));
            }
            continue;
        }
        let Record::Depth { raw, .. } = record else { unreachable!() };
        let update = parse_depth_update(&raw).expect("fixture event parses");
        match sync.as_mut() {
            None => early.push(update),
            Some(s) => match s.on_event(&mut book, update) {
                Outcome::Applied => {
                    out.insert(book.last_update_id, top(&book));
                }
                Outcome::Skipped => {}
                other => panic!("unexpected {other:?} on a clean recording"),
            },
        }
    }
    out
}

fn top(book: &OrderBook) -> (Levels, Levels) {
    (
        book.top_bids(TOP).into_iter().map(|l| (l.price, l.quantity)).collect(),
        book.top_asks(TOP).into_iter().map(|l| (l.price, l.quantity)).collect(),
    )
}

#[test]
fn live_recordings_replay_without_gaps() {
    for (name, rec, events, skipped) in [("A", A, 24, 1), ("B", B, 13, 1)] {
        let r = replay_reader(rec.as_bytes()).unwrap();
        assert_eq!(r.snapshots, 1, "{name}");
        assert_eq!(r.events, events, "{name}");
        assert_eq!((r.gaps, r.bad_lines), (0, 0), "{name}");
        assert_eq!(r.skipped, skipped, "{name}: the event before the snapshot is inside it");
        assert_eq!(r.applied, events - skipped, "{name}");
        assert_eq!(r.last_update_id, Some(LAST_ID), "{name}");
        let (bid, ask) = (r.best_bid.unwrap(), r.best_ask.unwrap());
        assert!(bid.0 < ask.0, "{name}: crossed book {bid:?} {ask:?}");
    }
}

#[test]
fn books_from_independent_snapshots_agree_at_every_shared_update() {
    let a = books_by_update(A);
    let b = books_by_update(B);
    let shared: Vec<u64> = b.keys().filter(|u| a.contains_key(u)).copied().collect();
    assert!(shared.len() >= 12, "only {} shared update ids", shared.len());
    assert_eq!(shared.last(), Some(&LAST_ID));
    for u in shared {
        assert_eq!(a[&u], b[&u], "books differ at update {u}");
        assert_eq!(a[&u].0.len(), TOP);
        assert_eq!(a[&u].1.len(), TOP);
    }
}

#[test]
fn a_dropped_message_is_detected_as_a_gap() {
    // Remove one depth event from the middle of recording A.
    let lines: Vec<&str> = A.lines().collect();
    for drop in [3, 10, 20] {
        assert!(lines[drop].contains("\"kind\":\"depth\""));
        let cut: String = lines
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != drop)
            .map(|(_, l)| format!("{l}\n"))
            .collect();
        let r = replay_reader(cut.as_bytes()).unwrap();
        assert_eq!(r.gaps, 1, "dropping line {drop}");
        assert_eq!(r.last_update_id, None, "a book with a gap must not report prices");
        assert_eq!(r.best_bid, None);
    }
}

#[test]
fn redelivered_messages_are_skipped_without_changing_the_book() {
    let doubled: String = A
        .lines()
        .flat_map(|l| {
            let n = if l.contains("\"kind\":\"depth\"") { 2 } else { 1 };
            std::iter::repeat_n(format!("{l}\n"), n)
        })
        .collect();
    let once = replay_reader(A.as_bytes()).unwrap();
    let twice = replay_reader(doubled.as_bytes()).unwrap();
    assert_eq!(twice.gaps, 0);
    assert_eq!(twice.applied, once.applied);
    assert_eq!(twice.skipped, once.skipped + once.events);
    assert_eq!(
        (twice.best_bid, twice.best_ask, twice.bid_depth, twice.ask_depth, twice.last_update_id),
        (once.best_bid, once.best_ask, once.bid_depth, once.ask_depth, once.last_update_id)
    );
}
