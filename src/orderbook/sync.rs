//! Binance's rules for stitching a REST depth snapshot to the WebSocket diff
//! stream, as pure functions so they can be tested exhaustively.
//!
//! Spot and USD-M futures use different sequencing:
//!
//! - **Spot** ([docs](https://developers.binance.com/docs/binance-spot-api-docs/web-socket-streams#how-to-manage-a-local-order-book-correctly)):
//!   drop events with `u <= lastUpdateId`; the first applied event has
//!   `U <= lastUpdateId + 1`. Afterwards ignore events with `u <=` the local
//!   update id, and resync if `U > local + 1`.
//! - **USD-M futures** ([docs](https://developers.binance.com/docs/derivatives/usds-margined-futures/websocket-market-streams/How-to-manage-a-local-order-book-correctly)):
//!   drop events with `u < lastUpdateId`; the first applied event has
//!   `U <= lastUpdateId` and `u >= lastUpdateId`. Afterwards each event's
//!   `pu` must equal the previous event's `u`.

use crate::ws::binance::DepthUpdate;

/// Which Binance market the book follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Market {
    /// Spot (`api.binance.com`, `stream.binance.com`).
    Spot,
    /// USD-M perpetual futures (`fapi.binance.com`, `fstream.binance.com`).
    UsdM,
}

impl Market {
    /// Parse `spot` or `usdm` (case-insensitive; `futures` is accepted for `usdm`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "spot" => Some(Self::Spot),
            "usdm" | "usd-m" | "futures" => Some(Self::UsdM),
            _ => None,
        }
    }

    /// Lower-case name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spot => "spot",
            Self::UsdM => "usdm",
        }
    }

    /// Default REST base URL.
    pub fn default_rest_base(self) -> &'static str {
        match self {
            Self::Spot => "https://api.binance.com",
            Self::UsdM => "https://fapi.binance.com",
        }
    }

    /// Path of the depth snapshot endpoint.
    pub fn depth_path(self) -> &'static str {
        match self {
            Self::Spot => "/api/v3/depth",
            Self::UsdM => "/fapi/v1/depth",
        }
    }

    /// Largest snapshot the endpoint serves.
    pub fn max_depth_limit(self) -> u32 {
        match self {
            Self::Spot => 5000,
            Self::UsdM => 1000,
        }
    }

    /// Default diff-depth WebSocket URL for `symbol`.
    pub fn default_ws_url(self, symbol: &str) -> String {
        let s = symbol.to_ascii_lowercase();
        match self {
            Self::Spot => format!("wss://stream.binance.com:9443/ws/{s}@depth"),
            Self::UsdM => format!("wss://fstream.binance.com/ws/{s}@depth@100ms"),
        }
    }

    /// Default trade WebSocket URL (for the VWAP) for `symbol`. Futures
    /// publish aggregated trades, which carry the same price, quantity and
    /// time fields.
    pub fn default_trade_ws_url(self, symbol: &str) -> String {
        let s = symbol.to_ascii_lowercase();
        match self {
            Self::Spot => format!("wss://stream.binance.com:9443/ws/{s}@trade"),
            Self::UsdM => format!("wss://fstream.binance.com/ws/{s}@aggTrade"),
        }
    }
}

/// What to do with a buffered event while connecting to a fresh snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handshake {
    /// Already contained in the snapshot: drop it.
    Stale,
    /// The first event to apply on top of the snapshot.
    Apply,
    /// Events between the snapshot and this one were missed: fetch a new snapshot.
    Gap,
}

/// Decide what to do with `update` while connecting to a snapshot taken at
/// `snapshot_id` (its `lastUpdateId`).
pub fn handshake(market: Market, snapshot_id: u64, update: &DepthUpdate) -> Handshake {
    match market {
        Market::Spot => {
            if update.last_update_id <= snapshot_id {
                Handshake::Stale
            } else if update.first_update_id > snapshot_id + 1 {
                Handshake::Gap
            } else {
                Handshake::Apply
            }
        }
        Market::UsdM => {
            if update.last_update_id < snapshot_id {
                Handshake::Stale
            } else if update.first_update_id > snapshot_id {
                Handshake::Gap
            } else {
                Handshake::Apply
            }
        }
    }
}

/// What to do with a live event, given the `u` of the last applied event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Live {
    /// Continues the sequence: apply it.
    Apply,
    /// Already applied (a duplicate or late event): skip it without resyncing.
    Ignore,
    /// An update was missed: the book is wrong until a resync.
    Gap {
        /// The id the next event had to start from (spot) or point back to (futures).
        expected: u64,
        /// What the event carried.
        got: u64,
    },
}

/// Decide what to do with a live `update` after the event whose `u` was `prev_last`.
pub fn live(market: Market, prev_last: u64, update: &DepthUpdate) -> Live {
    match market {
        Market::Spot => {
            if update.last_update_id <= prev_last {
                Live::Ignore
            } else if update.first_update_id > prev_last + 1 {
                Live::Gap {
                    expected: prev_last + 1,
                    got: update.first_update_id,
                }
            } else {
                Live::Apply
            }
        }
        Market::UsdM => {
            if update.last_update_id <= prev_last {
                Live::Ignore
            } else {
                match update.prev_last_update_id {
                    Some(pu) if pu == prev_last => Live::Apply,
                    Some(pu) => Live::Gap {
                        expected: prev_last,
                        got: pu,
                    },
                    // A futures event without `pu` cannot be checked: treat it
                    // like a gap rather than risk a silently wrong book.
                    None => Live::Gap {
                        expected: prev_last,
                        got: 0,
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(first: u64, last: u64, pu: Option<u64>) -> DepthUpdate {
        DepthUpdate {
            first_update_id: first,
            last_update_id: last,
            prev_last_update_id: pu,
            bids: vec![],
            asks: vec![],
        }
    }

    // ── spot handshake ──

    #[test]
    fn spot_drops_events_inside_the_snapshot() {
        assert_eq!(
            handshake(Market::Spot, 100, &ev(90, 100, None)),
            Handshake::Stale
        );
    }

    #[test]
    fn spot_applies_an_event_that_overlaps_or_follows_the_snapshot() {
        assert_eq!(
            handshake(Market::Spot, 100, &ev(95, 105, None)),
            Handshake::Apply
        );
        assert_eq!(
            handshake(Market::Spot, 100, &ev(101, 105, None)),
            Handshake::Apply
        );
    }

    #[test]
    fn spot_detects_a_gap_after_the_snapshot() {
        assert_eq!(
            handshake(Market::Spot, 100, &ev(102, 105, None)),
            Handshake::Gap
        );
    }

    // ── spot live ──

    #[test]
    fn spot_live_applies_the_next_event() {
        assert_eq!(live(Market::Spot, 105, &ev(106, 110, None)), Live::Apply);
    }

    #[test]
    fn spot_live_ignores_a_duplicate_instead_of_resyncing() {
        // Re-delivered or late events are harmless; resyncing on them costs a
        // snapshot (weight 250 at limit 5000) for nothing.
        assert_eq!(live(Market::Spot, 110, &ev(106, 110, None)), Live::Ignore);
        assert_eq!(live(Market::Spot, 110, &ev(100, 104, None)), Live::Ignore);
    }

    #[test]
    fn spot_live_detects_a_missed_event() {
        assert_eq!(
            live(Market::Spot, 105, &ev(107, 110, None)),
            Live::Gap {
                expected: 106,
                got: 107
            }
        );
    }

    // ── futures handshake ──

    #[test]
    fn futures_drops_events_strictly_before_the_snapshot() {
        assert_eq!(
            handshake(Market::UsdM, 100, &ev(90, 99, Some(89))),
            Handshake::Stale
        );
    }

    #[test]
    fn futures_applies_the_event_that_covers_the_snapshot_id() {
        // U <= lastUpdateId <= u. Note u == lastUpdateId is NOT stale on futures.
        assert_eq!(
            handshake(Market::UsdM, 100, &ev(95, 100, Some(94))),
            Handshake::Apply
        );
        assert_eq!(
            handshake(Market::UsdM, 100, &ev(98, 103, Some(97))),
            Handshake::Apply
        );
    }

    #[test]
    fn futures_detects_a_gap_after_the_snapshot() {
        // Spot would accept U == lastUpdateId + 1; futures requires U <= lastUpdateId.
        assert_eq!(
            handshake(Market::UsdM, 100, &ev(101, 105, Some(100))),
            Handshake::Gap
        );
    }

    // ── futures live ──

    #[test]
    fn futures_live_follows_pu_not_u_plus_one() {
        // Futures ids are not contiguous: the next event's U can jump, but its
        // pu must equal the previous u.
        assert_eq!(
            live(Market::UsdM, 105, &ev(120, 130, Some(105))),
            Live::Apply
        );
        assert_eq!(
            live(Market::UsdM, 105, &ev(106, 110, Some(104))),
            Live::Gap {
                expected: 105,
                got: 104
            }
        );
    }

    #[test]
    fn futures_live_without_pu_is_treated_as_a_gap() {
        assert!(matches!(
            live(Market::UsdM, 105, &ev(106, 110, None)),
            Live::Gap { .. }
        ));
    }

    #[test]
    fn market_names_parse() {
        assert_eq!(Market::parse("SPOT"), Some(Market::Spot));
        assert_eq!(Market::parse("usdm"), Some(Market::UsdM));
        assert_eq!(Market::parse("futures"), Some(Market::UsdM));
        assert_eq!(Market::parse("coinm"), None);
    }
}
