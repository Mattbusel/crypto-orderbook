//! Property tests for gap handling in the Coinbase and Kraken syncs, against
//! reference models of each venue's stream.
//!
//! **Coinbase.** One connection carries a `level2` snapshot, then a mix of
//! `level2` updates for the followed product, updates for another product
//! and heartbeats, every frame numbered by `sequence_num`. Frames may be
//! dropped or delivered twice. The rules checked:
//!
//! - whenever the sync is live, its book is the true book as of the last
//!   sequence number it accepted;
//! - the first frame after a lost one, whatever its channel, resyncs with
//!   exactly `expected` = the lost number and `got` = its own;
//! - a repeated frame changes nothing;
//! - after a resync a fresh snapshot (resubscribe) brings back the true book.
//!
//! **Kraken.** A full book (deeper than the subscription) changes one level
//! at a time. Each change becomes a v2 `update` the way Kraken sends them:
//! levels that change inside the subscribed depth, `qty` 0 for deleted
//! levels, the level that moves into view when one is deleted, nothing for
//! levels pushed out of view, and the CRC32 of the true top 10 written with
//! the pair's precision. The checksum string and CRC32 here are computed
//! independently of the library. The rules checked:
//!
//! - a clean stream is applied message by message, never resyncs, and the
//!   book equals the true subscribed view after every message;
//! - with messages lost, an applied message always leaves the true top 10
//!   (the checksummed levels), and a resync always leaves an empty book;
//! - a wrong checksum resyncs on that very message;
//! - a new snapshot always recovers.

use std::collections::BTreeMap;

use proptest::prelude::*;
use rust_decimal::Decimal;

use crypto_orderbook::orderbook::book::PriceLevel;
use crypto_orderbook::venue::{Book, CoinbaseSync, FeedOutcome, GapReason, KrakenSync, VenueSync};
use crypto_orderbook::OrderBook;

type Levels = BTreeMap<i64, u32>;

fn levels_of(book: &OrderBook, n: usize) -> (Vec<PriceLevel>, Vec<PriceLevel>) {
    (Book::top_bids(book, n), Book::top_asks(book, n))
}

/// Bids and asks as `PriceLevel`s (price = tick / 10), best first.
fn expected(bids: &Levels, asks: &Levels, n: usize) -> (Vec<PriceLevel>, Vec<PriceLevel>) {
    let lv = |(&t, &q): (&i64, &u32)| PriceLevel {
        price: Decimal::new(t, 1),
        quantity: Decimal::from(q),
    };
    (
        bids.iter().rev().take(n).map(lv).collect(),
        asks.iter().take(n).map(lv).collect(),
    )
}

// ── Coinbase ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum CbFrame {
    /// Level changes for BTC-USD: (bid?, tick, qty).
    Update(Vec<(bool, u8, u8)>),
    /// An update for another product.
    Other,
    /// A heartbeat.
    Heartbeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Once,
    Lost,
    Twice,
}

fn cb_tick(bid: bool, t: u8) -> i64 {
    if bid {
        1000 + i64::from(t)
    } else {
        1100 + i64::from(t)
    }
}

fn cb_json(seq: u64, kind: &str, product: &str, levels: &[(bool, i64, u32)]) -> String {
    let ups: Vec<String> = levels
        .iter()
        .map(|&(bid, t, q)| {
            format!(
                r#"{{"side":"{}","event_time":"2026-01-01T00:00:00Z","price_level":"{}","new_quantity":"{q}"}}"#,
                if bid { "bid" } else { "offer" },
                Decimal::new(t, 1)
            )
        })
        .collect();
    format!(
        r#"{{"channel":"l2_data","client_id":"","timestamp":"2026-01-01T00:00:00Z","sequence_num":{seq},"events":[{{"type":"{kind}","product_id":"{product}","updates":[{}]}}]}}"#,
        ups.join(",")
    )
}

fn cb_heartbeat(seq: u64) -> String {
    format!(
        r#"{{"channel":"heartbeats","client_id":"","timestamp":"2026-01-01T00:00:00Z","sequence_num":{seq},"events":[{{"current_time":"x","heartbeat_counter":"{seq}"}}]}}"#
    )
}

fn cb_snapshot(seq: u64, bids: &Levels, asks: &Levels) -> String {
    let all: Vec<(bool, i64, u32)> = bids
        .iter()
        .map(|(&t, &q)| (true, t, q))
        .chain(asks.iter().map(|(&t, &q)| (false, t, q)))
        .collect();
    cb_json(seq, "snapshot", "BTC-USD", &all)
}

fn cb_frame() -> impl Strategy<Value = CbFrame> {
    prop_oneof![
        6 => prop::collection::vec((any::<bool>(), 0u8..20, 0u8..4), 1..4).prop_map(CbFrame::Update),
        1 => Just(CbFrame::Other),
        2 => Just(CbFrame::Heartbeat),
    ]
}

/// Run the scenario and check the Coinbase rules.
fn check_coinbase(
    init: &[(bool, u8, u8)],
    frames: &[CbFrame],
    delivery: &[Delivery],
) -> Result<(), TestCaseError> {
    let (mut bids, mut asks) = (Levels::new(), Levels::new());
    for &(bid, t, q) in init {
        let side = if bid { &mut bids } else { &mut asks };
        if q > 0 {
            side.insert(cb_tick(bid, t), u32::from(q));
        }
    }

    // Build the wire frames (seq 0 = snapshot) and the true book after each.
    let mut wire = vec![cb_snapshot(0, &bids, &asks)];
    let mut truth = vec![(bids.clone(), asks.clone())];
    for (i, f) in frames.iter().enumerate() {
        let seq = i as u64 + 1;
        wire.push(match f {
            CbFrame::Update(changes) => {
                let lv: Vec<(bool, i64, u32)> = changes
                    .iter()
                    .map(|&(b, t, q)| (b, cb_tick(b, t), u32::from(q)))
                    .collect();
                for &(b, t, q) in &lv {
                    let side = if b { &mut bids } else { &mut asks };
                    if q == 0 {
                        side.remove(&t);
                    } else {
                        side.insert(t, q);
                    }
                }
                cb_json(seq, "update", "BTC-USD", &lv)
            }
            CbFrame::Other => cb_json(seq, "update", "ETH-USD", &[(true, 5, 1)]),
            CbFrame::Heartbeat => cb_heartbeat(seq),
        });
        truth.push((bids.clone(), asks.clone()));
    }

    let mut book = OrderBook::new();
    let mut sync = CoinbaseSync::new("BTC-USD");
    // The snapshot always arrives.
    prop_assert_eq!(
        sync.on_frame(&mut book, &wire[0]).unwrap(),
        FeedOutcome::Snapshot
    );
    let mut last_delivered = 0u64;
    let mut lost_since_last = false;
    let mut resynced = false;
    for seq in 1..wire.len() {
        let how = delivery[(seq - 1) % delivery.len()];
        if how == Delivery::Lost {
            lost_since_last = true;
            continue;
        }
        let was_live = sync.is_live();
        let outcome = sync.on_frame(&mut book, &wire[seq]).unwrap();
        if lost_since_last && was_live {
            prop_assert_eq!(
                outcome,
                FeedOutcome::Resync(GapReason::Sequence {
                    expected: last_delivered + 1,
                    got: seq as u64
                })
            );
            resynced = true;
        } else if !was_live {
            prop_assert!(
                matches!(outcome, FeedOutcome::Skipped | FeedOutcome::Ignored),
                "{:?}",
                outcome
            );
        }
        lost_since_last = false;
        last_delivered = seq as u64;
        if how == Delivery::Twice {
            prop_assert_eq!(
                sync.on_frame(&mut book, &wire[seq]).unwrap(),
                FeedOutcome::Skipped
            );
        }
        if sync.is_live() {
            let (b, a) = &truth[seq];
            prop_assert_eq!(levels_of(&book, 100), expected(b, a, 100), "seq {}", seq);
        } else {
            prop_assert_eq!(book.bid_depth() + book.ask_depth(), 0);
        }
    }
    prop_assert_eq!(resynced, sync.stats().resyncs > 0);

    // Resubscribe: a fresh snapshot continues the connection's numbering.
    let next = wire.len() as u64 + u64::from(lost_since_last);
    let (b, a) = truth.last().unwrap();
    let out = sync.on_frame(&mut book, &cb_snapshot(next, b, a)).unwrap();
    prop_assert_eq!(out, FeedOutcome::Snapshot);
    prop_assert_eq!(levels_of(&book, 100), expected(b, a, 100));
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn coinbase_clean_and_duplicated_streams_stay_exact(
        init in prop::collection::vec((any::<bool>(), 0u8..20, 1u8..4), 0..20),
        frames in prop::collection::vec(cb_frame(), 1..60),
        delivery in prop::collection::vec(prop_oneof![4 => Just(Delivery::Once), 1 => Just(Delivery::Twice)], 1..8),
    ) {
        check_coinbase(&init, &frames, &delivery)?;
    }

    #[test]
    fn coinbase_lost_frames_are_caught_on_the_next_frame(
        init in prop::collection::vec((any::<bool>(), 0u8..20, 1u8..4), 0..20),
        frames in prop::collection::vec(cb_frame(), 2..60),
        delivery in prop::collection::vec(
            prop_oneof![5 => Just(Delivery::Once), 1 => Just(Delivery::Lost), 1 => Just(Delivery::Twice)], 1..12),
    ) {
        check_coinbase(&init, &frames, &delivery)?;
    }
}

// ── Kraken ───────────────────────────────────────────────────────────────────

/// Bitwise CRC32 (IEEE), deliberately not the library's table version.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Kraken's checksum of the true book, precision (1, 8): a price tick `t`
/// is written `t` (one decimal place, point removed) and a quantity `q`
/// is written `q` followed by eight zeros.
fn true_checksum(bids: &Levels, asks: &Levels) -> u32 {
    let mut s = String::new();
    for (t, q) in asks.iter().take(10) {
        s.push_str(&format!("{t}{q}00000000"));
    }
    for (t, q) in bids.iter().rev().take(10) {
        s.push_str(&format!("{t}{q}00000000"));
    }
    crc32(s.as_bytes())
}

fn view(side: &Levels, bid: bool, depth: usize) -> Levels {
    if bid {
        side.iter()
            .rev()
            .take(depth)
            .map(|(&t, &q)| (t, q))
            .collect()
    } else {
        side.iter().take(depth).map(|(&t, &q)| (t, q)).collect()
    }
}

fn kr_levels(levels: &[(i64, u32)]) -> String {
    levels
        .iter()
        .map(|&(t, q)| format!(r#"{{"price":{},"qty":{q}}}"#, Decimal::new(t, 1)))
        .collect::<Vec<_>>()
        .join(",")
}

fn kr_msg(kind: &str, bids: &[(i64, u32)], asks: &[(i64, u32)], checksum: u32) -> String {
    format!(
        r#"{{"channel":"book","type":"{kind}","data":[{{"symbol":"BTC/USD","bids":[{}],"asks":[{}],"checksum":{checksum},"timestamp":"2026-01-01T00:00:00Z"}}]}}"#,
        kr_levels(bids),
        kr_levels(asks)
    )
}

/// What Kraken sends for one side when the full book goes from `old` to `new`.
fn side_diff(old_full: &Levels, new_full: &Levels, bid: bool, depth: usize) -> Vec<(i64, u32)> {
    let (old_v, new_v) = (view(old_full, bid, depth), view(new_full, bid, depth));
    let mut out = Vec::new();
    for t in old_v.keys() {
        if !new_full.contains_key(t) {
            out.push((*t, 0));
        }
    }
    for (t, q) in &new_v {
        if old_v.get(t) != Some(q) {
            out.push((*t, *q));
        }
    }
    out
}

struct KrStream {
    /// Frame text, and the true (bids, asks) full book after it.
    frames: Vec<(String, Levels, Levels)>,
}

fn kraken_stream(init: &[(bool, u8, u8)], changes: &[(bool, u8, u8)], depth: usize) -> KrStream {
    let tick = |bid: bool, t: u8| {
        if bid {
            1000 + i64::from(t)
        } else {
            1100 + i64::from(t)
        }
    };
    let (mut bids, mut asks) = (Levels::new(), Levels::new());
    for &(bid, t, q) in init {
        if q > 0 {
            if bid { &mut bids } else { &mut asks }.insert(tick(bid, t), u32::from(q));
        }
    }
    let snap_b: Vec<_> = view(&bids, true, depth).into_iter().rev().collect();
    let snap_a: Vec<_> = view(&asks, false, depth).into_iter().collect();
    let mut frames = vec![(
        kr_msg("snapshot", &snap_b, &snap_a, true_checksum(&bids, &asks)),
        bids.clone(),
        asks.clone(),
    )];
    for &(bid, t, q) in changes {
        let (old_b, old_a) = (bids.clone(), asks.clone());
        let side = if bid { &mut bids } else { &mut asks };
        if q == 0 {
            side.remove(&tick(bid, t));
        } else {
            side.insert(tick(bid, t), u32::from(q));
        }
        let (db, da) = (
            side_diff(&old_b, &bids, true, depth),
            side_diff(&old_a, &asks, false, depth),
        );
        if db.is_empty() && da.is_empty() {
            continue; // outside the subscribed view: Kraken sends nothing
        }
        frames.push((
            kr_msg("update", &db, &da, true_checksum(&bids, &asks)),
            bids.clone(),
            asks.clone(),
        ));
    }
    KrStream { frames }
}

fn kr_init() -> impl Strategy<Value = Vec<(bool, u8, u8)>> {
    prop::collection::vec((any::<bool>(), 0u8..60, 1u8..6), 0..80)
}

fn kr_changes() -> impl Strategy<Value = Vec<(bool, u8, u8)>> {
    prop::collection::vec((any::<bool>(), 0u8..60, 0u8..6), 1..120)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn kraken_clean_stream_matches_the_true_view_after_every_message(
        init in kr_init(), changes in kr_changes(), deep in any::<bool>(),
    ) {
        let depth = if deep { 25 } else { 10 };
        let stream = kraken_stream(&init, &changes, depth);
        let mut book = OrderBook::new();
        let mut sync = KrakenSync::new("BTC/USD", depth, 1, 8);
        for (i, (frame, b, a)) in stream.frames.iter().enumerate() {
            let outcome = sync.on_frame(&mut book, frame).unwrap();
            prop_assert_eq!(outcome, if i == 0 { FeedOutcome::Snapshot } else { FeedOutcome::Applied });
            prop_assert_eq!(levels_of(&book, 1000), expected(b, a, depth));
        }
        prop_assert_eq!(sync.stats().resyncs, 0);
    }

    #[test]
    fn kraken_lost_messages_never_leave_a_wrong_checksummed_book(
        init in kr_init(), changes in kr_changes(), deep in any::<bool>(),
        lost in prop::collection::vec(any::<bool>(), 1..10),
    ) {
        let depth = if deep { 25 } else { 10 };
        let stream = kraken_stream(&init, &changes, depth);
        let mut book = OrderBook::new();
        let mut sync = KrakenSync::new("BTC/USD", depth, 1, 8);
        for (i, (frame, b, a)) in stream.frames.iter().enumerate() {
            if i > 0 && lost[i % lost.len()] && i % 3 == 0 {
                continue;
            }
            match sync.on_frame(&mut book, frame).unwrap() {
                FeedOutcome::Snapshot | FeedOutcome::Applied => {
                    prop_assert_eq!(levels_of(&book, 10), expected(b, a, 10), "frame {}", i);
                }
                FeedOutcome::Resync(GapReason::Checksum { expected: e, computed }) => {
                    prop_assert_eq!(e, true_checksum(b, a));
                    prop_assert_ne!(e, computed);
                    prop_assert_eq!(book.bid_depth() + book.ask_depth(), 0);
                    // Resubscribe: Kraken sends the current view as a snapshot.
                    let snap_b: Vec<_> = view(b, true, depth).into_iter().rev().collect();
                    let snap_a: Vec<_> = view(a, false, depth).into_iter().collect();
                    let snap = kr_msg("snapshot", &snap_b, &snap_a, true_checksum(b, a));
                    prop_assert_eq!(sync.on_frame(&mut book, &snap).unwrap(), FeedOutcome::Snapshot);
                    prop_assert_eq!(levels_of(&book, 1000), expected(b, a, depth));
                }
                FeedOutcome::Skipped => prop_assert!(sync.needs_snapshot()),
                other => prop_assert!(false, "unexpected {:?}", other),
            }
        }
    }

    #[test]
    fn kraken_wrong_checksum_resyncs_on_that_message(
        init in kr_init(), changes in kr_changes(), deep in any::<bool>(), at in any::<prop::sample::Index>(),
    ) {
        let depth = if deep { 25 } else { 10 };
        let stream = kraken_stream(&init, &changes, depth);
        let bad = at.index(stream.frames.len());
        let mut book = OrderBook::new();
        let mut sync = KrakenSync::new("BTC/USD", depth, 1, 8);
        for (i, (frame, b, a)) in stream.frames.iter().enumerate() {
            let truth = true_checksum(b, a);
            if i == bad {
                let wrong = truth.wrapping_add(1);
                let frame = frame.replace(&format!(r#""checksum":{truth}"#), &format!(r#""checksum":{wrong}"#));
                prop_assert_eq!(
                    sync.on_frame(&mut book, &frame).unwrap(),
                    FeedOutcome::Resync(GapReason::Checksum { expected: wrong, computed: truth })
                );
                prop_assert!(sync.needs_snapshot());
                break;
            }
            prop_assert!(!sync.on_frame(&mut book, frame).unwrap().is_resync());
        }
    }
}
