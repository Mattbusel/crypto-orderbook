use std::collections::VecDeque;
use std::time::Duration;

use rust_decimal::Decimal;

/// An entry in the sliding VWAP window.
struct Entry {
    price:        Decimal,
    quantity:     Decimal,
    timestamp_ms: u64,
}

/// Rolling VWAP (Volume Weighted Average Price) over a sliding time window.
///
/// Maintains a running sum of price*quantity and quantity so that
/// each new trade is O(1) to add and each evicted trade is O(1) to remove.
/// No full re-scan on every update.
pub struct VwapWindow {
    window:  Duration,
    entries: VecDeque<Entry>,
    sum_pq:  Decimal,
    sum_q:   Decimal,
}

impl VwapWindow {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            entries: VecDeque::new(),
            sum_pq:  Decimal::ZERO,
            sum_q:   Decimal::ZERO,
        }
    }

    /// Add a trade and evict entries that have aged out of the window.
    pub fn add(&mut self, price: Decimal, quantity: Decimal, timestamp_ms: u64) {
        self.evict(timestamp_ms);
        self.sum_pq += price * quantity;
        self.sum_q  += quantity;
        self.entries.push_back(Entry { price, quantity, timestamp_ms });
    }

    /// Current VWAP. None if no trades in the window.
    pub fn vwap(&self) -> Option<Decimal> {
        if self.sum_q.is_zero() { None } else { Some(self.sum_pq / self.sum_q) }
    }

    /// Total volume traded in the current window.
    pub fn volume(&self) -> Decimal {
        self.sum_q
    }

    fn evict(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(self.window.as_millis() as u64);
        while let Some(front) = self.entries.front() {
            if front.timestamp_ms < cutoff {
                let e = self.entries.pop_front().unwrap();
                self.sum_pq -= e.price * e.quantity;
                self.sum_q  -= e.quantity;
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn vwap_weighted_correctly() {
        let mut w = VwapWindow::new(Duration::from_secs(60));
        // 1 BTC at 100, 2 BTC at 200 -> VWAP = (100+400)/3 = 166.666...
        w.add(dec!(100), dec!(1), 1000);
        w.add(dec!(200), dec!(2), 2000);
        let v = w.vwap().unwrap();
        assert_eq!(v, dec!(500) / dec!(3));
    }

    #[test]
    fn old_entries_evicted() {
        let mut w = VwapWindow::new(Duration::from_millis(5000));
        w.add(dec!(100), dec!(1), 0);     // t=0, will be evicted
        w.add(dec!(200), dec!(1), 6000);  // t=6s, now_ms=6000 -> cutoff=1000 -> evicts t=0
        // only the 200 trade should be in the window
        assert_eq!(w.vwap().unwrap(), dec!(200));
    }

    #[test]
    fn empty_window_returns_none() {
        let w = VwapWindow::new(Duration::from_secs(60));
        assert!(w.vwap().is_none());
    }
}
