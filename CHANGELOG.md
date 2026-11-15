# Changelog

## [0.5.0] - 2026-11-15

### Added
- **Coinbase Advanced Trade `level2`** (`venue::CoinbaseSync`): the same sans-IO design as `BookSync`. Snapshot and update events with absolute quantities (`"0"` removes), the per-connection `sequence_num` checked on every frame of the connection (heartbeats and subscription acks included), a jump while live drops the book with `GapReason::Sequence { expected, got }`, repeated or late frames are skipped, other products are ignored. `subscribe_message()` / `unsubscribe_message()` for the resubscribe that brings a fresh snapshot; `reset()` after a reconnect. `parse_coinbase_message` for the frames.
- **Kraken WebSocket v2 `book`** (`venue::KrakenSync`): snapshot and updates applied in order, the book truncated to the subscribed depth after each message (Kraken sends no removals for levels pushed out of view), and the CRC32 checksum of the top 10 levels checked after every message; a mismatch drops the book with `GapReason::Checksum { expected, computed }`. Prices and quantities are read from the JSON text without going through `f64` and written with the pair's `price_precision` / `qty_precision`, which Kraken's own book channel example needs (`0.566` must be written `0.5660`). `kraken_checksum`, `kraken_checksum_input`, `parse_kraken_message`, and `parse_kraken_instruments` to read each pair's precision from the `instrument` channel.
- **One API across venues** (`venue` module, re-exported at the crate root): the `Book` trait (best bid and ask, top levels, depth, plus derived spread, mid, micro-price, imbalance and `is_crossed`), implemented by `OrderBook`; the `VenueSync` trait (`on_frame` with raw text frames, `is_live`, `needs_snapshot`, `snapshot_source`, `stats`, `reset`), implemented by `BookSync`, `CoinbaseSync` and `KrakenSync`; `Venue`, `FeedOutcome`, `GapReason` and `SnapshotSource`. Through `VenueSync`, `BookSync::on_frame` takes both diff-depth frames and REST snapshot bodies.
- `OrderBook::set_level` and `OrderBook::truncate`, the `Side` enum, and `PartialEq` on `orderbook::book::PriceLevel`.
- `examples/follow_venue.rs`: one loop for Coinbase or Kraken through `VenueSync`, resubscribing on a resync, optionally recording every frame. Checked live on 2026-11-15: Kraken BTC/USD depth 10 (40 updates) and ETH/USD depth 25 (300 updates) with every checksum matching and 0 resyncs; Coinbase BTC-USD 100 updates in sequence, 0 resyncs.
- **Replay tests** (`tests/venue_replay.rs`) on the venues' documentation samples: Kraken's book channel snapshot and update (checksums 2439117997 and 2114181697) and its checksum guide example (the guide's exact input string and CRC32 3310070434), Coinbase's `level2` snapshot and `heartbeats` examples. Plus the live recordings above: every Kraken update dropped in turn never leaves a wrong top 10 behind an applied message, and every Coinbase frame dropped in turn is caught on the next frame with the exact sequence numbers.
- **Property tests** (`tests/venue_properties.rs`) for gap handling against reference models: Coinbase streams with lost and repeated frames across channels, and Kraken streams generated from a full book deeper than the subscription (with an independent checksum implementation), with lost messages and wrong checksums. Each of four deliberately broken rules (no truncation, ignoring sequence gaps, skipping the checksum, applying updates before a snapshot) fails at least one of them.

### Changed
- `Market` derives `Hash`. `serde_json` is built with `raw_value`.

## [0.4.0] - 2026-11-14

### Added
- **Library API.** `BookSync` runs Binance's whole local order book procedure as a sans-IO state machine: buffer events, `needs_snapshot()`, `snapshot_is_too_old()` (refetch), `on_snapshot()` (drop stale events, connect the first one), `on_event()` (continuity: spot `U == previous u + 1`, futures `pu == previous u`; duplicates skipped), and `Outcome::Resync` with a `ResyncReason` on a gap. Bring your own WebSocket and HTTP client. Also `parse_depth_snapshot`, `DepthSnapshot`, `SyncState`, `SyncStats`, crate-root re-exports, crate docs with a doctest, and `examples/follow_book.rs` (checked live against Binance.US: 5 events applied, 0 resyncs).
- **One implementation of the rules.** The service's manager and the offline replay both drive `BookSync`, so the library, the service and `crypto-orderbook replay` cannot disagree. The binary now uses the library crate instead of compiling its own copy of the modules.
- **Property tests** (`tests/sync_properties.rs`, proptest, 512 cases each) against a reference model of spot (contiguous ids) and futures (ids with holes, `pu`) streams. After every call the book must equal the true book at the id `BookSync` claims. They cover clean streams with re-delivered duplicates, every possible snapshot age, a dropped event at every position (resync on the very next event, then recovery from a new snapshot), and wrong or missing futures `pu`. Each of five deliberately broken rules (for example, accepting `U == lastUpdateId + 2`, or ignoring `pu`) fails at least one of them.
- **Replay tests on real recorded messages** (`tests/replay_recorded.rs`): two Binance.US BTCUSDT recordings made by the service from separate connections and separate snapshots. The two books must be identical (top 20 levels each side) at all 13 update ids they share, a dropped message must be reported as a gap, and re-delivered messages must change nothing.
- **Metrics:** `orderbook_synced`, `orderbook_last_update_id`, `orderbook_buffered_events`, `orderbook_resyncs_total`, `orderbook_events_skipped_total`, `orderbook_parse_errors_total`, `orderbook_snapshot_refetches_total`, `orderbook_snapshot_failures_total`. `metrics::router()` serves `/metrics` alone, for programs that embed the library.

### Changed
- After a gap, the event that revealed it is kept as the first buffered event and the next snapshot is fetched at once. Before, the manager waited for another event first.
- A spot snapshot whose `lastUpdateId` is exactly `U - 1` of the first buffered event is used instead of being fetched again (spot ids are contiguous, so nothing is missing). Binance's current wording would refetch it; the module docs explain the difference.

### Fixed
- `orderbook_ws_reconnects_total` was never incremented. `orderbook_ws_events_total` only counted applied events, not received ones. `orderbook_snapshot_rate_limited_total` and `orderbook_rest_used_weight_1m` were missing from `/metrics` until their first use.
- Sequence id arithmetic can no longer overflow near `u64::MAX`.

## [0.3.0] - 2026-10-04

### Added
- **USD-M perpetual futures** (`MARKET=usdm`): fapi/fstream endpoints and Binance's futures sequencing (`pu` must equal the previous event's `u`; the first event must cover the snapshot id), with unit tests for both markets' rules.
- **Record and replay**: `RECORD_FILE` writes every snapshot and raw depth event (NDJSON); `crypto-orderbook replay FILE` rebuilds the book offline with the same rules. Checked on a live Binance.US recording: the replayed book at update 3354085117 matched what the service served.
- `DEPTH_LIMIT` for the snapshot size, and the `orderbook_rest_used_weight_1m` and `orderbook_snapshot_rate_limited_total` metrics.

### Fixed
- **Rate limits**: the snapshot request ignored the HTTP status, so a `429` showed up as "JSON parse failed" and was retried on a short backoff, which on Binance leads to an IP ban (`418`). It now waits for `Retry-After`.
- **One snapshot per sync, not two**: the snapshot was fetched before the stream delivered anything, so it was usually older than the first event and every start or reconnect needed a second snapshot (250 weight each at limit 5000). It now follows Binance's order: wait for the first event, then snapshot (measured live: 2 snapshots before, 1 after).
- **Duplicate events no longer force a resync**: on spot, an event already applied (`u <=` the last applied id) is skipped as Binance's procedure says, instead of being treated as a gap.
- Sequencing rules moved into `orderbook::sync` as pure functions with tests (there were none for the manager before).

## [0.2.1] - 2026-09-30

- TLS through rustls instead of the system OpenSSL, so one Linux build runs on every distribution (checked live against Binance.US).
- Linux x86_64 release builds from GitLab CI; downloads and links now point at GitLab.

## 0.2.0 (2026-09-25)

- Prebuilt binaries for Windows, macOS (Apple Silicon and Intel) and Linux on every GitHub Release, with SHA256SUMS.txt.
- `--help` and `--version` flags; logs default to `info` so a plain run shows what it is doing.
- `REST_BASE` is now honoured by the snapshot request, so the service works against Binance.US where binance.com returns 451.
- CI workflow (build and test).
- Crate metadata for crates.io (`cargo install crypto-orderbook`).

## 0.1.0

- Initial release: Binance snapshot-plus-diff order book, REST, WebSocket push and Prometheus metrics.
