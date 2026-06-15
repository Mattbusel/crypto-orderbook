use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::ws::binance::DepthUpdate;

/// The in-memory order book for a single trading pair.
///
/// Bids and asks are both stored as BTreeMap<price, quantity>.
/// BTreeMap keeps entries sorted by price automatically, which means:
///   - best ask = first entry in asks (lowest seller)
///   - best bid = last entry in bids (highest buyer)
/// Both are O(log n) to find, update, and remove.
pub struct OrderBook {
    /// Buyers: maps price -> quantity. Sorted ascending, so best bid is at the back.
    bids: BTreeMap<Decimal, Decimal>,

    /// Sellers: maps price -> quantity. Sorted ascending, so best ask is at the front.
    asks: BTreeMap<Decimal, Decimal>,

    /// The last update ID we successfully applied.
    /// The manager uses this to detect sequence gaps.
    pub last_update_id: u64,
}

impl OrderBook {
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_update_id: 0,
        }
    }

    /// Wipe all price levels and reset the sequence counter.
    /// Called by the manager when a reconnect or sequence gap requires a full resync.
    pub fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.last_update_id = 0;
    }

    /// Apply a parsed depth update from Binance.
    ///
    /// For each price level in the update:
    ///   - quantity > 0: insert or overwrite the level at that price
    ///   - quantity == 0: remove the level entirely (Binance's deletion signal)
    ///
    /// This does not validate sequence numbers. That is the manager's job.
    /// This function only mutates the book.
    pub fn apply(&mut self, update: &DepthUpdate) {
        apply_levels(&mut self.bids, &update.bids);
        apply_levels(&mut self.asks, &update.asks);
        self.last_update_id = update.last_update_id;
    }

    /// The highest price any buyer is currently willing to pay.
    /// Returns None if the book has no bids yet.
    pub fn best_bid(&self) -> Option<PriceLevel> {
        self.bids.iter().next_back().map(|(&price, &quantity)| PriceLevel { price, quantity })
    }

    /// The lowest price any seller is currently willing to accept.
    /// Returns None if the book has no asks yet.
    pub fn best_ask(&self) -> Option<PriceLevel> {
        self.asks.iter().next().map(|(&price, &quantity)| PriceLevel { price, quantity })
    }

    /// The gap between the best ask and the best bid.
    /// Returns None if either side of the book is empty.
    pub fn spread(&self) -> Option<Decimal> {
        let bid = self.best_bid()?.price;
        let ask = self.best_ask()?.price;
        Some(ask - bid)
    }

    /// The top `n` bid levels, ordered from highest price to lowest.
    /// This is what a typical market depth display shows on the buy side.
    pub fn top_bids(&self, n: usize) -> Vec<PriceLevel> {
        self.bids
            .iter()
            .rev()
            .take(n)
            .map(|(&price, &quantity)| PriceLevel { price, quantity })
            .collect()
    }

    /// The top `n` ask levels, ordered from lowest price to highest.
    /// This is what a typical market depth display shows on the sell side.
    pub fn top_asks(&self, n: usize) -> Vec<PriceLevel> {
        self.asks
            .iter()
            .take(n)
            .map(|(&price, &quantity)| PriceLevel { price, quantity })
            .collect()
    }

    /// Total number of bid price levels currently in the book.
    pub fn bid_depth(&self) -> usize {
        self.bids.len()
    }

    /// Total number of ask price levels currently in the book.
    pub fn ask_depth(&self) -> usize {
        self.asks.len()
    }
}

/// A single price level returned by queries on the book.
/// Separate from the parser's PriceLevel so the book has no dependency on
/// the Binance wire format.
#[derive(Debug, Clone, Copy)]
pub struct PriceLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

/// Apply a slice of price level updates to one side of the book (bids or asks).
/// Extracted as a function because the logic is identical for both sides.
fn apply_levels(map: &mut BTreeMap<Decimal, Decimal>, levels: &[crate::ws::binance::PriceLevel]) {
    for level in levels {
        if level.is_removal() {
            map.remove(&level.price);
        } else {
            map.insert(level.price, level.quantity);
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::binance::{DepthUpdate, PriceLevel as ParsedLevel};
    use rust_decimal_macros::dec;

    fn make_update(
        first: u64,
        last: u64,
        bids: Vec<(Decimal, Decimal)>,
        asks: Vec<(Decimal, Decimal)>,
    ) -> DepthUpdate {
        DepthUpdate {
            first_update_id: first,
            last_update_id: last,
            bids: bids.into_iter().map(|(p, q)| ParsedLevel { price: p, quantity: q }).collect(),
            asks: asks.into_iter().map(|(p, q)| ParsedLevel { price: p, quantity: q }).collect(),
        }
    }

    #[test]
    fn best_bid_is_highest_buyer() {
        let mut book = OrderBook::new();
        book.apply(&make_update(1, 1,
            vec![(dec!(100), dec!(1)), (dec!(101), dec!(2)), (dec!(99), dec!(3))],
            vec![],
        ));
        assert_eq!(book.best_bid().unwrap().price, dec!(101));
    }

    #[test]
    fn best_ask_is_lowest_seller() {
        let mut book = OrderBook::new();
        book.apply(&make_update(1, 1,
            vec![],
            vec![(dec!(102), dec!(1)), (dec!(103), dec!(2)), (dec!(104), dec!(3))],
        ));
        assert_eq!(book.best_ask().unwrap().price, dec!(102));
    }

    #[test]
    fn zero_quantity_removes_level() {
        let mut book = OrderBook::new();
        book.apply(&make_update(1, 1,
            vec![(dec!(100), dec!(5))],
            vec![],
        ));
        assert_eq!(book.bid_depth(), 1);

        book.apply(&make_update(2, 2,
            vec![(dec!(100), dec!(0))],
            vec![],
        ));
        assert_eq!(book.bid_depth(), 0);
        assert!(book.best_bid().is_none());
    }

    #[test]
    fn spread_is_ask_minus_bid() {
        let mut book = OrderBook::new();
        book.apply(&make_update(1, 1,
            vec![(dec!(100), dec!(1))],
            vec![(dec!(102), dec!(1))],
        ));
        assert_eq!(book.spread().unwrap(), dec!(2));
    }

    #[test]
    fn top_bids_ordered_highest_first() {
        let mut book = OrderBook::new();
        book.apply(&make_update(1, 1,
            vec![(dec!(100), dec!(1)), (dec!(101), dec!(2)), (dec!(99), dec!(3))],
            vec![],
        ));
        let top = book.top_bids(3);
        assert_eq!(top[0].price, dec!(101));
        assert_eq!(top[1].price, dec!(100));
        assert_eq!(top[2].price, dec!(99));
    }

    #[test]
    fn clear_resets_everything() {
        let mut book = OrderBook::new();
        book.apply(&make_update(1, 5,
            vec![(dec!(100), dec!(1))],
            vec![(dec!(101), dec!(1))],
        ));
        book.clear();
        assert_eq!(book.bid_depth(), 0);
        assert_eq!(book.ask_depth(), 0);
        assert_eq!(book.last_update_id, 0);
    }
}
