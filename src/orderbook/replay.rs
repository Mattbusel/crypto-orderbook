//! Record the raw feed to a file and rebuild the book from it offline.
//!
//! A recording is newline-delimited JSON: one line per REST snapshot and one
//! per raw depth event, each with the time it was received. Replaying it runs
//! the same Binance sequencing rules as the live service
//! ([`super::sync`]), so a recording reproduces the book exactly, gaps and
//! resyncs included. Use it to backtest against real books or to debug a
//! session after the fact.
//!
//! ```text
//! RECORD_FILE=btc.ndjson crypto-orderbook        # record while serving
//! crypto-orderbook replay btc.ndjson              # rebuild offline
//! ```

use std::io::{BufRead, Write};
use std::path::Path;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::orderbook::book::OrderBook;
use crate::orderbook::sync::{self, Handshake, Live, Market};
use crate::ws::binance::{parse_depth_update, DepthUpdate, PriceLevel};

/// One line of a recording.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Record {
    /// A REST snapshot the service applied.
    Snapshot {
        /// Milliseconds since the Unix epoch when it was received.
        t: u64,
        /// `spot` or `usdm`.
        market: String,
        /// The snapshot's `lastUpdateId`.
        last_update_id: u64,
        /// Bid levels as `[price, quantity]` strings.
        bids: Vec<[String; 2]>,
        /// Ask levels as `[price, quantity]` strings.
        asks: Vec<[String; 2]>,
    },
    /// A raw depth event exactly as the WebSocket delivered it.
    Depth {
        /// Milliseconds since the Unix epoch when it was received.
        t: u64,
        /// The frame's JSON text.
        raw: String,
    },
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn levels_to_strings(levels: &[PriceLevel]) -> Vec<[String; 2]> {
    levels
        .iter()
        .map(|l| [l.price.to_string(), l.quantity.to_string()])
        .collect()
}

/// Appends records to a file, one JSON line each, flushed per line so a
/// stopped process loses nothing.
pub struct Recorder {
    out: std::io::BufWriter<std::fs::File>,
}

impl Recorder {
    /// Open `path` for appending.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self {
            out: std::io::BufWriter::new(file),
        })
    }

    fn write(&mut self, record: &Record) -> std::io::Result<()> {
        serde_json::to_writer(&mut self.out, record)?;
        self.out.write_all(b"\n")?;
        self.out.flush()
    }

    /// Record an applied snapshot.
    pub fn snapshot(
        &mut self,
        market: Market,
        last_update_id: u64,
        bids: &[PriceLevel],
        asks: &[PriceLevel],
    ) -> std::io::Result<()> {
        self.write(&Record::Snapshot {
            t: now_ms(),
            market: market.as_str().to_string(),
            last_update_id,
            bids: levels_to_strings(bids),
            asks: levels_to_strings(asks),
        })
    }

    /// Record a raw depth frame.
    pub fn depth(&mut self, raw: &str) -> std::io::Result<()> {
        self.write(&Record::Depth {
            t: now_ms(),
            raw: raw.to_string(),
        })
    }
}

/// What a replay found.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ReplayReport {
    /// Snapshots in the recording.
    pub snapshots: usize,
    /// Depth events in the recording.
    pub events: usize,
    /// Events applied to the book.
    pub applied: usize,
    /// Events skipped as already contained or duplicated.
    pub skipped: usize,
    /// Sequence gaps (each would have forced a resync live).
    pub gaps: usize,
    /// Lines that could not be parsed.
    pub bad_lines: usize,
    /// Best bid and ask at the end, if the book was synced.
    pub best_bid: Option<(Decimal, Decimal)>,
    /// Best ask at the end.
    pub best_ask: Option<(Decimal, Decimal)>,
    /// Price levels per side at the end.
    pub bid_depth: usize,
    /// Ask levels at the end.
    pub ask_depth: usize,
    /// The update id the book had reached at the end, if synced.
    pub last_update_id: Option<u64>,
}

#[derive(PartialEq)]
enum State {
    /// No snapshot yet, or the book went out of sequence.
    Unsynced,
    /// Snapshot applied, waiting for the first event that connects.
    Handshake { snapshot_id: u64 },
    /// Following the stream.
    Live { prev: u64 },
}

/// Rebuild the book from a recording made by [`Recorder`].
pub fn replay(path: &Path) -> std::io::Result<ReplayReport> {
    let file = std::fs::File::open(path)?;
    let mut report = ReplayReport::default();
    let mut book = OrderBook::new();
    let mut market = Market::Spot;
    let mut state = State::Unsynced;

    for line in std::io::BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: Record = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => {
                report.bad_lines += 1;
                continue;
            }
        };
        match record {
            Record::Snapshot {
                market: m,
                last_update_id,
                bids,
                asks,
                ..
            } => {
                report.snapshots += 1;
                market = Market::parse(&m).unwrap_or(Market::Spot);
                let parse = |levels: Vec<[String; 2]>| -> Vec<PriceLevel> {
                    levels
                        .into_iter()
                        .filter_map(|[p, q]| {
                            Some(PriceLevel {
                                price: p.parse().ok()?,
                                quantity: q.parse().ok()?,
                            })
                        })
                        .collect()
                };
                book.clear();
                book.apply(&DepthUpdate {
                    first_update_id: last_update_id,
                    last_update_id,
                    prev_last_update_id: None,
                    bids: parse(bids),
                    asks: parse(asks),
                });
                state = State::Handshake {
                    snapshot_id: last_update_id,
                };
            }
            Record::Depth { raw, .. } => {
                report.events += 1;
                let Ok(update) = parse_depth_update(&raw) else {
                    report.bad_lines += 1;
                    continue;
                };
                match state {
                    State::Unsynced => report.skipped += 1,
                    State::Handshake { snapshot_id } => {
                        match sync::handshake(market, snapshot_id, &update) {
                            Handshake::Stale => report.skipped += 1,
                            Handshake::Gap => {
                                report.gaps += 1;
                                book.clear();
                                state = State::Unsynced;
                            }
                            Handshake::Apply => {
                                book.apply(&update);
                                report.applied += 1;
                                state = State::Live {
                                    prev: update.last_update_id,
                                };
                            }
                        }
                    }
                    State::Live { prev } => match sync::live(market, prev, &update) {
                        Live::Apply => {
                            book.apply(&update);
                            report.applied += 1;
                            state = State::Live {
                                prev: update.last_update_id,
                            };
                        }
                        Live::Ignore => report.skipped += 1,
                        Live::Gap { .. } => {
                            report.gaps += 1;
                            book.clear();
                            state = State::Unsynced;
                        }
                    },
                }
            }
        }
    }

    if let State::Live { prev } = state {
        report.last_update_id = Some(prev);
        report.best_bid = book.best_bid().map(|l| (l.price, l.quantity));
        report.best_ask = book.best_ask().map(|l| (l.price, l.quantity));
        report.bid_depth = book.bid_depth();
        report.ask_depth = book.ask_depth();
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth(first: u64, last: u64, bids: &str, asks: &str) -> String {
        format!(
            r#"{{"e":"depthUpdate","E":1,"s":"BTCUSDT","U":{first},"u":{last},"b":{bids},"a":{asks}}}"#
        )
    }

    fn level(p: &str, q: &str) -> PriceLevel {
        PriceLevel {
            price: p.parse().unwrap(),
            quantity: q.parse().unwrap(),
        }
    }

    #[test]
    fn a_recording_replays_to_the_same_book() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.ndjson");
        let mut rec = Recorder::open(&path).unwrap();
        rec.depth(&depth(95, 100, "[]", "[]")).unwrap(); // before the snapshot: stale
        rec.snapshot(
            Market::Spot,
            100,
            &[level("100.0", "1.0"), level("99.0", "2.0")],
            &[level("101.0", "1.5")],
        )
        .unwrap();
        rec.depth(&depth(95, 100, "[]", "[]")).unwrap(); // stale after the snapshot
        rec.depth(&depth(101, 103, r#"[["100.5","0.7"]]"#, "[]"))
            .unwrap(); // new best bid
        rec.depth(&depth(
            104,
            105,
            r#"[["99.0","0"]]"#,
            r#"[["101.0","0.5"]]"#,
        ))
        .unwrap(); // removal
        rec.depth(&depth(104, 105, "[]", "[]")).unwrap(); // duplicate: ignored
        drop(rec);

        let r = replay(&path).unwrap();
        assert_eq!((r.snapshots, r.events, r.applied, r.gaps), (1, 5, 2, 0));
        assert_eq!(r.skipped, 3);
        assert_eq!(
            r.best_bid,
            Some(("100.5".parse().unwrap(), "0.7".parse().unwrap()))
        );
        assert_eq!(
            r.best_ask,
            Some(("101.0".parse().unwrap(), "0.5".parse().unwrap()))
        );
        assert_eq!((r.bid_depth, r.ask_depth), (2, 1));
    }

    #[test]
    fn a_gap_in_the_recording_is_reported_and_unsyncs_the_book() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.ndjson");
        let mut rec = Recorder::open(&path).unwrap();
        rec.snapshot(
            Market::Spot,
            100,
            &[level("100", "1")],
            &[level("101", "1")],
        )
        .unwrap();
        rec.depth(&depth(101, 102, "[]", "[]")).unwrap();
        rec.depth(&depth(110, 111, "[]", "[]")).unwrap(); // missed 103..=109
        drop(rec);
        let r = replay(&path).unwrap();
        assert_eq!(r.gaps, 1);
        assert_eq!(r.best_bid, None, "an unsynced book reports no prices");
    }

    #[test]
    fn bad_lines_are_counted_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.ndjson");
        std::fs::write(
            &path,
            "not json\n\n{\"kind\":\"depth\",\"t\":1,\"raw\":\"{}\"}\n",
        )
        .unwrap();
        let r = replay(&path).unwrap();
        assert_eq!(r.bad_lines, 2);
    }
}
