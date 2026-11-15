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
use crate::orderbook::sync::{BookSync, Market, Outcome};
use crate::ws::binance::{parse_depth_update, DepthSnapshot, PriceLevel};

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

impl Record {
    /// The snapshot as a [`DepthSnapshot`] and its market, if this is a
    /// snapshot record. Levels that do not parse are dropped.
    pub fn to_snapshot(&self) -> Option<(Market, DepthSnapshot)> {
        let Record::Snapshot {
            market,
            last_update_id,
            bids,
            asks,
            ..
        } = self
        else {
            return None;
        };
        let parse = |levels: &[[String; 2]]| -> Vec<PriceLevel> {
            levels
                .iter()
                .filter_map(|[p, q]| {
                    Some(PriceLevel {
                        price: p.parse().ok()?,
                        quantity: q.parse().ok()?,
                    })
                })
                .collect()
        };
        Some((
            Market::parse(market).unwrap_or(Market::Spot),
            DepthSnapshot {
                last_update_id: *last_update_id,
                bids: parse(bids),
                asks: parse(asks),
            },
        ))
    }
}

/// Rebuild the book from a recording made by [`Recorder`].
pub fn replay(path: &Path) -> std::io::Result<ReplayReport> {
    let file = std::fs::File::open(path)?;
    replay_reader(std::io::BufReader::new(file))
}

/// Rebuild the book from a recording read from `reader`, running it through
/// [`BookSync`] exactly as the live service does: depth lines before a
/// snapshot are buffered, the snapshot line replays the buffer onto itself,
/// and every later event is checked for continuity.
pub fn replay_reader<R: BufRead>(reader: R) -> std::io::Result<ReplayReport> {
    let mut report = ReplayReport::default();
    let mut book = OrderBook::new();
    let mut sync: Option<BookSync> = None;
    // Depth lines before the first snapshot line: the market is not known
    // until then, so they are held and handed to BookSync with it.
    let mut early = Vec::new();

    for line in reader.lines() {
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
        match &record {
            Record::Snapshot { .. } => {
                let Some((market, snapshot)) = record.to_snapshot() else {
                    continue;
                };
                report.snapshots += 1;
                let s = sync.get_or_insert_with(|| BookSync::new(market));
                for update in early.drain(..) {
                    s.on_event(&mut book, update);
                }
                let done = s.on_snapshot(&mut book, snapshot);
                report.applied += done.applied;
                report.skipped += done.skipped;
                if done.resync.is_some() {
                    report.gaps += 1;
                }
            }
            Record::Depth { raw, .. } => {
                report.events += 1;
                let Ok(update) = parse_depth_update(raw) else {
                    report.bad_lines += 1;
                    continue;
                };
                let Some(s) = sync.as_mut() else {
                    early.push(update);
                    continue;
                };
                match s.on_event(&mut book, update) {
                    Outcome::Applied => report.applied += 1,
                    Outcome::Skipped => report.skipped += 1,
                    Outcome::Buffered => {}
                    Outcome::Resync(_) => report.gaps += 1,
                }
            }
        }
    }

    if sync.as_ref().is_some_and(BookSync::is_live) {
        report.last_update_id = Some(book.last_update_id);
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
