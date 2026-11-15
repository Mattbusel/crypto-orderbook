//! Property tests for Binance's local order book procedure as implemented by
//! `BookSync`, against a reference model.
//!
//! The model: a stream of single-level changes, one per update id. Spot ids
//! are contiguous; USD-M futures ids are increasing with holes (as on
//! Binance, where `U` does not follow the previous `u`). Events batch
//! consecutive changes the way Binance does (one entry per price, the last
//! quantity wins) and carry `U`, `u` and, on futures, `pu`. A snapshot at
//! update id `S` is the true book after the change with id `S`.
//!
//! Safety invariant, checked after every call: whatever state `BookSync`
//! reports, the book is the true book at that id (Live at `u`, Handshake at
//! the snapshot id) or empty (Buffering). It may refuse to sync, but it may
//! never hold a wrong book while claiming to be in sync.

use std::collections::BTreeMap;

use proptest::prelude::*;
use rust_decimal::Decimal;

use crypto_orderbook::{
    BookSync, DepthSnapshot, DepthUpdate, Market, OrderBook, Outcome, PriceLevel, ResyncReason, SyncState,
};

type Side = BTreeMap<Decimal, Decimal>;

#[derive(Debug, Clone)]
struct Change {
    bid: bool,
    price: u8,
    qty: u8,
}

impl Change {
    fn price(&self) -> Decimal {
        Decimal::from(if self.bid { 100 } else { 200 } + u32::from(self.price))
    }
}

#[derive(Debug)]
struct Model {
    market: Market,
    /// Update id of each change.
    ids: Vec<u64>,
    /// The true book after each change.
    truth: Vec<(Side, Side)>,
    events: Vec<DepthUpdate>,
    /// Index of each event's first change.
    event_start: Vec<usize>,
}

impl Model {
    fn build(market: Market, changes: &[Change], cuts: &[bool], steps: &[u8]) -> Self {
        let mut ids = Vec::with_capacity(changes.len());
        let mut id = 1_000u64;
        for (i, _) in changes.iter().enumerate() {
            ids.push(id);
            id += match market {
                Market::Spot => 1,
                Market::UsdM => u64::from(steps[i % steps.len()]),
            };
        }

        let mut truth = Vec::with_capacity(changes.len());
        let (mut bids, mut asks) = (Side::new(), Side::new());
        for c in changes {
            let side = if c.bid { &mut bids } else { &mut asks };
            if c.qty == 0 {
                side.remove(&c.price());
            } else {
                side.insert(c.price(), Decimal::from(c.qty));
            }
            truth.push((bids.clone(), asks.clone()));
        }

        let mut events = Vec::new();
        let mut event_start = Vec::new();
        let mut start = 0;
        let mut prev_u = ids[0] - 1;
        for i in 0..changes.len() {
            if !(cuts[i % cuts.len()] || i + 1 == changes.len()) {
                continue;
            }
            let (mut b, mut a): (BTreeMap<Decimal, Decimal>, BTreeMap<Decimal, Decimal>) = Default::default();
            for c in &changes[start..=i] {
                let side = if c.bid { &mut b } else { &mut a };
                side.insert(c.price(), Decimal::from(c.qty));
            }
            let levels = |m: BTreeMap<Decimal, Decimal>| {
                m.into_iter().map(|(price, quantity)| PriceLevel { price, quantity }).collect()
            };
            events.push(DepthUpdate {
                first_update_id: ids[start],
                last_update_id: ids[i],
                prev_last_update_id: (market == Market::UsdM).then_some(prev_u),
                bids: levels(b),
                asks: levels(a),
            });
            event_start.push(start);
            prev_u = ids[i];
            start = i + 1;
        }
        Self { market, ids, truth, events, event_start }
    }

    fn snapshot(&self, k: usize) -> DepthSnapshot {
        let (b, a) = &self.truth[k];
        let levels = |m: &Side| m.iter().map(|(&price, &quantity)| PriceLevel { price, quantity }).collect();
        DepthSnapshot { last_update_id: self.ids[k], bids: levels(b), asks: levels(a) }
    }

    fn truth_at(&self, id: u64) -> &(Side, Side) {
        let k = self.ids.binary_search(&id).expect("state id is an update id");
        &self.truth[k]
    }

    /// The lowest change index a snapshot may be taken at and still connect
    /// to a stream whose first buffered event is `event`.
    fn lowest_connecting_snapshot(&self, event: usize) -> usize {
        let s = self.event_start[event];
        match self.market {
            Market::Spot => s.saturating_sub(1),
            Market::UsdM => s,
        }
    }

    /// Is `event` already contained in a snapshot taken at change `k`?
    fn stale(&self, event: usize, k: usize) -> bool {
        let (u, s) = (self.events[event].last_update_id, self.ids[k]);
        match self.market {
            Market::Spot => u <= s,
            Market::UsdM => u < s,
        }
    }
}

fn book_sides(book: &OrderBook) -> (Side, Side) {
    let b = book.top_bids(usize::MAX).into_iter().map(|l| (l.price, l.quantity)).collect();
    let a = book.top_asks(usize::MAX).into_iter().map(|l| (l.price, l.quantity)).collect();
    (b, a)
}

/// The safety invariant.
fn check(model: &Model, sync: &BookSync, book: &OrderBook) -> Result<(), TestCaseError> {
    match sync.state() {
        SyncState::Buffering => {
            prop_assert_eq!(book.bid_depth() + book.ask_depth(), 0, "buffering with a non-empty book");
        }
        SyncState::Handshake { snapshot_id } => {
            prop_assert_eq!(&book_sides(book), model.truth_at(snapshot_id), "handshake book != snapshot");
        }
        SyncState::Live { last_update_id } => {
            prop_assert_eq!(book.last_update_id, last_update_id);
            prop_assert_eq!(&book_sides(book), model.truth_at(last_update_id), "live book wrong at {}", last_update_id);
        }
    }
    Ok(())
}

fn change() -> impl Strategy<Value = Change> {
    (any::<bool>(), 0u8..10, 0u8..4).prop_map(|(bid, price, qty)| Change { bid, price, qty })
}

fn market() -> impl Strategy<Value = Market> {
    prop_oneof![Just(Market::Spot), Just(Market::UsdM)]
}

#[derive(Debug)]
struct Scenario {
    model: Model,
    /// Event the connection starts at.
    start: usize,
    /// Change index the snapshot is taken at (connects to `start`).
    k: usize,
    /// Events delivered before the snapshot is supplied (>= 1).
    before: usize,
    /// Random numbers for choices made inside a test.
    picks: Vec<u32>,
}

fn scenario() -> impl Strategy<Value = Scenario> {
    scenario_in(market())
}

fn scenario_in(market: impl Strategy<Value = Market>) -> impl Strategy<Value = Scenario> {
    (
        market,
        prop::collection::vec(change(), 2..120),
        prop::collection::vec(prop::bool::weighted(0.4), 1..16),
        prop::collection::vec(1u8..4, 1..16),
        any::<u32>(),
        any::<u32>(),
        any::<u32>(),
        prop::collection::vec(any::<u32>(), 8),
    )
        .prop_map(|(market, changes, cuts, steps, r_start, r_k, r_before, picks)| {
            let model = Model::build(market, &changes, &cuts, &steps);
            let n_events = model.events.len();
            let start = r_start as usize % n_events;
            let lo = model.lowest_connecting_snapshot(start);
            let k = lo + r_k as usize % (model.ids.len() - lo);
            let before = 1 + r_before as usize % (n_events - start);
            Scenario { model, start, k, before, picks }
        })
}

/// Feed `order` (event indices) with the snapshot after the first `before`
/// of them, checking the invariant after every call.
fn run(model: &Model, order: &[usize], before: usize, k: usize) -> Result<(BookSync, OrderBook, Vec<Outcome>), TestCaseError> {
    let mut sync = BookSync::new(model.market);
    let mut book = OrderBook::new();
    let mut outcomes = Vec::new();
    for (pos, &e) in order.iter().enumerate() {
        if pos == before {
            prop_assert!(!sync.snapshot_is_too_old(model.ids[k]));
            let done = sync.on_snapshot(&mut book, model.snapshot(k));
            prop_assert_eq!(done.resync, None);
            check(model, &sync, &book)?;
        }
        let out = sync.on_event(&mut book, model.events[e].clone());
        if pos < before {
            prop_assert_eq!(out, Outcome::Buffered);
        }
        outcomes.push(out);
        check(model, &sync, &book)?;
    }
    if order.len() == before {
        let done = sync.on_snapshot(&mut book, model.snapshot(k));
        prop_assert_eq!(done.resync, None);
        check(model, &sync, &book)?;
    }
    Ok((sync, book, outcomes))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    /// A clean stream (with re-delivered duplicates) syncs with no resync and
    /// ends with the true book, wherever the connection starts, whenever the
    /// snapshot is taken and however many events were buffered before it.
    #[test]
    fn clean_stream_converges_to_the_true_book(sc in scenario(), dup_every in 0usize..6) {
        let m = &sc.model;
        let mut order: Vec<usize> = Vec::new();
        for e in sc.start..m.events.len() {
            order.push(e);
            // Re-deliver an earlier event now and then: must be skipped.
            if dup_every > 0 && e % dup_every == 0 && e > sc.start {
                order.push(sc.start + sc.picks[0] as usize % (e - sc.start));
            }
        }
        let before = sc.before.min(order.len());
        let (sync, book, outcomes) = run(m, &order, before, sc.k)?;
        prop_assert!(!outcomes.iter().any(|o| matches!(o, Outcome::Resync(_))), "resync on a clean stream");
        prop_assert_eq!(sync.stats().resyncs, 0);
        let last = m.ids.len() - 1;
        // Live unless the snapshot already contains every event (spot only:
        // on futures the event with u == lastUpdateId is still applied).
        if m.market == Market::UsdM || m.ids[sc.k] < m.ids[last] {
            prop_assert!(sync.is_live());
            prop_assert_eq!(&book_sides(&book), &m.truth[last]);
            prop_assert_eq!(book.last_update_id, m.ids[last]);
        }
    }

    /// A snapshot older than the first buffered event is always detected,
    /// before it is used (`snapshot_is_too_old`) and when it is
    /// (`SnapshotTooOld`); a snapshot that connects is never rejected.
    #[test]
    fn too_old_snapshots_are_always_detected(sc in scenario()) {
        let m = &sc.model;
        let lo = m.lowest_connecting_snapshot(sc.start);
        for k in 0..m.ids.len() {
            let mut sync = BookSync::new(m.market);
            let mut book = OrderBook::new();
            sync.on_event(&mut book, m.events[sc.start].clone());
            prop_assert_eq!(sync.snapshot_is_too_old(m.ids[k]), k < lo, "k {} lo {}", k, lo);
            let done = sync.on_snapshot(&mut book, m.snapshot(k));
            if k < lo {
                let is_too_old = matches!(done.resync, Some(ResyncReason::SnapshotTooOld { .. }));
                prop_assert!(is_too_old);
                prop_assert!(sync.needs_snapshot());
            } else {
                prop_assert_eq!(done.resync, None);
            }
            check(m, &sync, &book)?;
        }
    }

    /// Dropping one event is harmless if the snapshot already contains it;
    /// otherwise the very next event triggers a resync, the book is never
    /// wrong in between, and a fresh snapshot recovers the true book.
    #[test]
    fn a_missed_event_forces_a_resync_and_a_new_snapshot_recovers(sc in scenario()) {
        let m = &sc.model;
        let n_events = m.events.len();
        prop_assume!(n_events - sc.start >= 2);
        // Drop an event that has a successor.
        let g = sc.start + sc.picks[1] as usize % (n_events - 1 - sc.start);
        let order: Vec<usize> = (sc.start..n_events).filter(|&e| e != g).collect();
        let before = sc.before.min(order.len());

        let mut sync = BookSync::new(m.market);
        let mut book = OrderBook::new();
        let mut resync_at = None;
        let mut snapshot_supplied = false;
        for (pos, &e) in order.iter().enumerate() {
            if pos == before {
                let done = sync.on_snapshot(&mut book, m.snapshot(sc.k));
                snapshot_supplied = true;
                if done.resync.is_some() {
                    // The buffered successor of the dropped event revealed it.
                    resync_at = Some(g + 1);
                }
                check(m, &sync, &book)?;
                if resync_at.is_some() { break; }
            }
            let out = sync.on_event(&mut book, m.events[e].clone());
            check(m, &sync, &book)?;
            if let Outcome::Resync(_) = out {
                resync_at = Some(e);
                break;
            }
        }
        if !snapshot_supplied && resync_at.is_none() {
            let done = sync.on_snapshot(&mut book, m.snapshot(sc.k));
            if done.resync.is_some() { resync_at = Some(g + 1); }
            check(m, &sync, &book)?;
        }

        if m.stale(g, sc.k) {
            prop_assert_eq!(resync_at, None, "dropping a stale event must not resync");
            return Ok(());
        }
        prop_assert_eq!(resync_at, Some(g + 1), "the event after the missed one must resync");
        prop_assert!(sync.needs_snapshot());
        prop_assert_eq!(sync.first_buffered_update_id(), Some(m.events[g + 1].first_update_id));

        // Resync: a new snapshot that connects to the buffered event, then
        // the rest of the stream.
        let lo = m.lowest_connecting_snapshot(g + 1);
        let k2 = lo + sc.picks[2] as usize % (m.ids.len() - lo);
        prop_assert!(!sync.snapshot_is_too_old(m.ids[k2]));
        let done = sync.on_snapshot(&mut book, m.snapshot(k2));
        prop_assert_eq!(done.resync, None);
        check(m, &sync, &book)?;
        for e in (g + 2)..n_events {
            let out = sync.on_event(&mut book, m.events[e].clone());
            prop_assert!(!matches!(out, Outcome::Resync(_)));
            check(m, &sync, &book)?;
        }
        prop_assert_eq!(sync.stats().resyncs, 1);
        if sync.is_live() {
            prop_assert_eq!(&book_sides(&book), m.truth.last().unwrap());
        }
    }

    /// On futures a wrong `pu` is a gap even when `U` looks contiguous, and
    /// a missing `pu` is never trusted.
    #[test]
    fn futures_continuity_is_checked_with_pu(sc in scenario_in(Just(Market::UsdM)), bad_pu in any::<u64>(), drop_pu in any::<bool>()) {
        let m = &sc.model;
        prop_assume!(m.events.len() - sc.start >= 2);
        let mut sync = BookSync::new(m.market);
        let mut book = OrderBook::new();
        sync.on_event(&mut book, m.events[sc.start].clone());
        let lo = m.lowest_connecting_snapshot(sc.start);
        sync.on_snapshot(&mut book, m.snapshot(lo));
        prop_assert!(sync.is_live());
        let prev = m.events[sc.start].last_update_id;
        let mut next = m.events[sc.start + 1].clone();
        prop_assume!(bad_pu != prev);
        next.first_update_id = prev + 1;
        next.prev_last_update_id = if drop_pu { None } else { Some(bad_pu) };
        let out = sync.on_event(&mut book, next);
        let is_gap = matches!(out, Outcome::Resync(ResyncReason::Gap { expected, .. }) if expected == prev);
        prop_assert!(is_gap);
        check(m, &sync, &book)?;
    }
}

/// A reconnect wipes the buffer and the book: events from the old
/// connection must never be stitched to the new one.
#[test]
fn reset_forgets_everything() {
    let model = Model::build(
        Market::Spot,
        &[Change { bid: true, price: 1, qty: 1 }, Change { bid: false, price: 1, qty: 2 }],
        &[true],
        &[1],
    );
    let mut sync = BookSync::new(Market::Spot);
    let mut book = OrderBook::new();
    sync.on_event(&mut book, model.events[0].clone());
    sync.on_snapshot(&mut book, model.snapshot(0));
    sync.on_event(&mut book, model.events[1].clone());
    assert!(sync.is_live());
    sync.reset(&mut book);
    assert_eq!(sync.state(), SyncState::Buffering);
    assert_eq!((sync.buffered(), book.bid_depth(), book.ask_depth()), (0, 0, 0));
    assert!(!sync.needs_snapshot());
}

/// A full buffer drops its oldest events, which can only make a snapshot
/// look too old, never let a gap through.
#[test]
fn buffer_overflow_drops_the_oldest_event() {
    let changes: Vec<Change> = (0..6).map(|i| Change { bid: true, price: i, qty: 1 }).collect();
    let model = Model::build(Market::Spot, &changes, &[true], &[1]);
    let mut sync = BookSync::new(Market::Spot).with_max_buffer(2);
    let mut book = OrderBook::new();
    for e in &model.events[..4] {
        assert_eq!(sync.on_event(&mut book, e.clone()), Outcome::Buffered);
    }
    assert_eq!(sync.buffered(), 2);
    assert_eq!(sync.stats().buffer_overflows, 2);
    assert_eq!(sync.first_buffered_update_id(), Some(model.events[2].first_update_id));
    // A snapshot from before the dropped events no longer connects.
    assert!(sync.snapshot_is_too_old(model.ids[0]));
    assert!(!sync.snapshot_is_too_old(model.ids[1]));
}
