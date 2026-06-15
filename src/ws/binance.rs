use rust_decimal::Decimal;
use serde::Deserialize;

use crate::error::{AppError, Result};

// ── Wire format ───────────────────────────────────────────────────────────────
//
// This is the exact shape Binance sends over the WebSocket.
// We keep these types private to this module — nothing outside sees them.
// The rest of the system works with `DepthUpdate`, our own domain type.
//
// Binance sends bids and asks as arrays of two-element string arrays:
//   "b": [["29500.00", "1.24"], ["29499.00", "0.00"]]
// Not as objects with named fields. So we deserialize each entry as a
// two-element tuple of strings, then parse strings into Decimal.

#[derive(Debug, Deserialize)]
struct RawDepthUpdate {
    // "U": first update ID covered by this event
    #[serde(rename = "U")]
    first_update_id: u64,

    // "u": last update ID covered by this event
    #[serde(rename = "u")]
    last_update_id: u64,

    // "b": bid levels to update — [price_str, quantity_str]
    // quantity "0" or "0.00000000" means: remove this price level entirely
    #[serde(rename = "b")]
    bids: Vec<[String; 2]>,

    // "a": ask levels to update — same format as bids
    #[serde(rename = "a")]
    asks: Vec<[String; 2]>,
}

// ── Domain types ──────────────────────────────────────────────────────────────
//
// These are what the rest of the system uses. They have no knowledge of
// Binance's JSON field names, wire format, or string encoding.
// If Binance renamed "b" to "bids" tomorrow, only this file changes.

/// A single price level update: a price and the new total quantity at that price.
/// If quantity is zero, this level should be removed from the book entirely.
#[derive(Debug, Clone)]
pub struct PriceLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

impl PriceLevel {
    /// Returns true if this update means "remove this price level from the book".
    /// Binance signals deletion by sending quantity zero, not by sending a
    /// separate delete message.
    pub fn is_removal(&self) -> bool {
        self.quantity.is_zero()
    }
}

/// A fully parsed depth update event, ready for the manager to apply to the book.
#[derive(Debug, Clone)]
pub struct DepthUpdate {
    /// The sequence ID of the first update bundled in this event.
    /// Used to verify we haven't missed any events.
    pub first_update_id: u64,

    /// The sequence ID of the last update bundled in this event.
    /// The next event we receive must have first_update_id == this + 1.
    pub last_update_id: u64,

    /// Bid levels to update or remove.
    pub bids: Vec<PriceLevel>,

    /// Ask levels to update or remove.
    pub asks: Vec<PriceLevel>,
}

// ── Parsing ───────────────────────────────────────────────────────────────────

/// Parse a raw JSON string from the WebSocket into a `DepthUpdate`.
///
/// This is the only entry point into this module. The manager calls this
/// with every `WsEvent::Message` it receives.
pub fn parse_depth_update(raw: &str) -> Result<DepthUpdate> {
    let raw: RawDepthUpdate = serde_json::from_str(raw)?;
    convert(raw)
}

fn convert(raw: RawDepthUpdate) -> Result<DepthUpdate> {
    Ok(DepthUpdate {
        first_update_id: raw.first_update_id,
        last_update_id: raw.last_update_id,
        bids: parse_levels(raw.bids)?,
        asks: parse_levels(raw.asks)?,
    })
}

/// Parse a list of [price_str, quantity_str] pairs into PriceLevels.
///
/// Why parse strings into Decimal here and not at the point of use?
/// Because the manager should never see raw strings. If a string fails to
/// parse as a valid number, that is a parse error — it belongs here, in the
/// layer whose job is turning Binance's format into our format.
fn parse_levels(raw: Vec<[String; 2]>) -> Result<Vec<PriceLevel>> {
    raw.into_iter().map(|entry| {
        let price = entry[0].parse::<Decimal>()
            .map_err(|e| AppError::InvalidField(
                format!("invalid price '{}': {}", entry[0], e)
            ))?;
        let quantity = entry[1].parse::<Decimal>()
            .map_err(|e| AppError::InvalidField(
                format!("invalid quantity '{}': {}", entry[1], e)
            ))?;
        Ok(PriceLevel { price, quantity })
    }).collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // A realistic Binance depthUpdate frame, trimmed for readability.
    const SAMPLE: &str = r#"{
        "e": "depthUpdate",
        "E": 1699000000000,
        "s": "BTCUSDT",
        "U": 100,
        "u": 103,
        "b": [
            ["29500.00000000", "1.24500000"],
            ["29499.50000000", "0.00000000"]
        ],
        "a": [
            ["29501.00000000", "0.75000000"]
        ]
    }"#;

    #[test]
    fn parses_sequence_ids() {
        let update = parse_depth_update(SAMPLE).unwrap();
        assert_eq!(update.first_update_id, 100);
        assert_eq!(update.last_update_id, 103);
    }

    #[test]
    fn parses_bid_price_and_quantity() {
        let update = parse_depth_update(SAMPLE).unwrap();
        assert_eq!(update.bids[0].price.to_string(), "29500.00000000");
        assert_eq!(update.bids[0].quantity.to_string(), "1.24500000");
    }

    #[test]
    fn detects_removal_by_zero_quantity() {
        let update = parse_depth_update(SAMPLE).unwrap();
        // second bid has quantity 0 — Binance's way of saying "delete this level"
        assert!(update.bids[1].is_removal());
        assert!(!update.bids[0].is_removal());
    }

    #[test]
    fn rejects_non_numeric_price() {
        let bad = r#"{"e":"depthUpdate","E":1,"s":"X","U":1,"u":1,"b":[["bad","1.0"]],"a":[]}"#;
        assert!(parse_depth_update(bad).is_err());
    }
}
