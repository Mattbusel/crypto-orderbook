# crypto-orderbook: your local order book is wrong after the first dropped message

Binance, Coinbase and Kraken do not send you an order book. They send a snapshot and a stream of diffs, and the book is only correct if every diff connects to the one before it. Drop one message on a reconnect and a book that applies whatever arrives keeps showing a bid that was cancelled, with nothing in the API to tell you. crypto-orderbook takes the other side: **a book that cannot prove its sequence is cleared, not served.** Every gap is a resync, every Kraken update is checksum-validated, and the state machine that decides this does no I/O, so you can drop it into whatever socket code you already have.

```sh
cargo run --example catch_a_gap --no-default-features
```
```text
snapshot 100 applied, 1 buffered event(s) replayed
best bid Some((64250.10, 0.50))
caught: Gap { expected: 103, got: 105 }
book cleared (0 bids), new snapshot needed: true
```

**Why it exists.** Binance's own documentation spells out a six-step handshake (buffer the stream, fetch the snapshot, drop events at or before `lastUpdateId`, find the first event that straddles it, then require `U == previous u + 1` on spot and `pu == previous u` on USD-M futures). Coinbase needs sequence numbers checked per product, Kraken a CRC32 over the top ten levels. A GitHub code search for Binance's `lastUpdateId` in Rust on Nov 20 2026 returns files from 68 repositories. Each one is a place this handshake gets written again.

**Why this instead of your own.** One tested state machine for four markets (Binance spot and USD-M, Coinbase Advanced Trade, Kraken v2), checked by property tests against reference models (each of nine deliberately broken rules, such as accepting `U == lastUpdateId + 2` or skipping Kraken's checksum, fails at least one) and by replays of real recorded streams, with exact `rust_decimal` prices, and a typed reason on every resync (`SnapshotTooOld`, `Gap { expected, got }`). It never applies an event it cannot place.

**Why today.** `default-features = false` gives you only the sync state machines, the book and the parsers, on four dependencies (serde, serde_json, rust_decimal, thiserror). No Tokio, no HTTP client, no web server unless you ask for the service.

**Who it is for:** Rust trading and market-data code that keeps its own L2 book from exchange WebSockets: market makers, arbitrage bots, recorders and terminals.

**Trade-offs, on purpose:** it will drop your book and make you refetch a snapshot rather than guess; it covers four venues and does not try to cover forty; prices are `Decimal`, never `f64`.

## Use it as a library

```toml
[dependencies]
crypto-orderbook = { version = "0.5", default-features = false }
```

Feed it the frames your own socket reads: `BookSync::on_event` for each diff, `on_snapshot` when `needs_snapshot()` says so. `examples/catch_a_gap.rs` is the whole loop without a network; `examples/follow_book.rs` runs it against live Binance, `examples/follow_venue.rs` against Coinbase and Kraken.

## Run it as a service

The default `service` feature adds a Binance service that keeps the book in memory and serves it over HTTP, WebSocket and Prometheus.

## Features

- **Binance depth sync** (`src/orderbook/manager.rs`): fetch a REST snapshot (`/api/v3/depth?limit=5000`), discard stale buffered diffs, find the first event that bridges the snapshot, then apply diffs in live mode with a sequence-continuity check. Any gap or reconnect clears the book and restarts the cycle.
- **Circuit breaker** on the snapshot request: opens after 5 consecutive failures, cools down for 30 s, then lets one probe through.
- **Resilient WebSocket client** (`src/ws/client.rs`): reconnects with exponential backoff (1 s doubling to a 30 s cap, plus jitter) and signals the manager so it resyncs.
- **Exact decimals**: prices and quantities are `rust_decimal::Decimal`, stored in `BTreeMap`s per side.
- **Derived values**: spread, micro-price (size-weighted mid), top-N depth imbalance, and a rolling 1-minute VWAP and volume from the `@trade` stream.
- **Push updates**: every applied diff broadcasts a compact top-of-book JSON message to `/ws/book` subscribers; slow clients skip ahead instead of buffering.
- **Prometheus metrics**: events, reconnects, snapshots, sequence gaps, updates applied, book depth, spread, VWAP and volume.

## Install

**Linux** (x86_64, Ubuntu 20.04+ / Debian 11+). One line, no dependencies, installs to `~/.local/bin`:

```sh
mkdir -p ~/.local/bin && curl -fsSL https://gitlab.com/mattbusel/crypto-orderbook/-/releases/permalink/latest/downloads/crypto-orderbook-linux-x86_64.tar.gz | tar xz --strip-components=1 -C ~/.local/bin --wildcards '*/crypto-orderbook'
```

| Other systems | |
|---|---|
| **Windows** | [Download crypto-orderbook-windows-x86_64.exe](https://gitlab.com/mattbusel/crypto-orderbook/-/releases/permalink/latest/downloads/crypto-orderbook-windows-x86_64.exe) and run it. (Unsigned, so SmartScreen may ask: *More info*, then *Run anyway*.) |
| **macOS, or from source** | `cargo install --locked crypto-orderbook` |

TLS is built in (rustls), so the Linux download needs no system OpenSSL. Every release, with SHA-256 checksums: [Releases](https://gitlab.com/mattbusel/crypto-orderbook/-/releases).

## Quick start

Start it with no arguments (`crypto-orderbook`, or `cargo run --release` from a clone). Then, once `/health` reports `synced: true`:

```sh
curl localhost:3000/health
curl localhost:3000/book/best
curl "localhost:3000/book/snapshot?depth=10"
curl "localhost:3000/book/imbalance?depth=20"
curl localhost:3000/book/vwap
curl localhost:3000/metrics
```

Or with Docker (multi-stage build into a distroless image):

```sh
docker build -t crypto-orderbook .
docker run --rm -p 3000:3000 -e SYMBOL=ETHUSDT crypto-orderbook
```

Run the integration tests (they start the Axum router on a random port against an in-memory book; no network access to Binance needed):

```sh
cargo test
```

## Configuration

All settings come from environment variables (`src/config.rs`).

| Variable | Default | Meaning |
|----------|---------|---------|
| `SYMBOL` | `BTCUSDT` | Trading pair to track |
| `MARKET` | `spot` | `spot`, or `usdm` for USD-M perpetual futures (fapi/fstream endpoints, `pu` sequencing, `@aggTrade` for the VWAP) |
| `DEPTH_LIMIT` | `5000` spot, `1000` usdm | Levels in the REST snapshot. On spot 5000 levels cost 250 of the 6000 request weight Binance allows a minute; 1000 cost 50 |
| `RECORD_FILE` | unset | Append every snapshot and raw depth event to this file (NDJSON) for `crypto-orderbook replay` |
| `API_PORT` | `3000` | HTTP port |
| `WS_BASE` | spot `wss://stream.binance.com:9443/ws/<symbol>@depth`, usdm `wss://fstream.binance.com/ws/<symbol>@depth@100ms` | Depth diff stream |
| `TRADE_WS_BASE` | spot `.../<symbol>@trade`, usdm `.../<symbol>@aggTrade` | Trade stream for VWAP |
| `REST_BASE` | spot `https://api.binance.com`, usdm `https://fapi.binance.com` | REST base for the depth snapshot |
| `CHANNEL_BUFFER` | `10000` | Events buffered while the snapshot is fetched |
| `RUST_LOG` | `info` | `tracing` filter, e.g. `debug` |

`binance.com` answers `451 Unavailable For Legal Reasons` from some regions, including the US. Point it at Binance.US instead:

```sh
REST_BASE=https://api.binance.us \
WS_BASE=wss://stream.binance.us:9443/ws/btcusdt@depth \
TRADE_WS_BASE=wss://stream.binance.us:9443/ws/btcusdt@trade \
crypto-orderbook
```

## API

| Endpoint | Returns |
|----------|---------|
| `GET /health` | `200` with depth and last update ID once synced, `503` while syncing |
| `GET /book/best` | best bid, best ask, spread, micro-price |
| `GET /book/snapshot?depth=N` | top N levels per side (default 20, capped at 100) |
| `GET /book/bids?depth=N`, `GET /book/asks?depth=N` | one side only |
| `GET /book/spread` | best ask minus best bid |
| `GET /book/midprice` | micro-price: `(ask * bid_qty + bid * ask_qty) / (bid_qty + ask_qty)` |
| `GET /book/imbalance?depth=N` | `(bid_qty - ask_qty) / (bid_qty + ask_qty)` over the top N levels, with a text label |
| `GET /book/vwap` | rolling 1-minute VWAP and volume |
| `GET /metrics` | Prometheus text format |
| `GET /ws/book` | WebSocket; one JSON top-of-book message per applied update |

Every request has a 10 second timeout.

## How it works

```
Binance @depth WS ─> ws::client ─mpsc─> orderbook::Manager ─> Arc<RwLock<OrderBook>> ─> Axum routes
                                            │  (snapshot, handshake, live)              
Binance REST depth ─────────────────────────┘                └─broadcast─> /ws/book subscribers
Binance @trade WS ─> ws::client ─mpsc─> trades::TradeManager ─> Arc<RwLock<VwapWindow>> ─> /book/vwap
```

| Path | Role |
|------|------|
| `src/main.rs` | wires channels, tasks and the HTTP server; graceful Ctrl-C |
| `src/ws/client.rs`, `src/ws/binance.rs` | WebSocket actor and Binance message parsing |
| `src/orderbook/manager.rs` | snapshot fetch, handshake, live sequencing, circuit breaker |
| `src/orderbook/book.rs` | `OrderBook`: BTreeMap sides, spread, micro-price, imbalance |
| `src/trades/` | trade stream consumer and time-windowed VWAP |
| `src/api/` | REST routes and WebSocket push |
| `src/metrics.rs` | Prometheus registry |
| `tests/api_integration.rs` | HTTP-level tests of the API |
| `docs/crypto_orderbook_design.pdf` | design document (generated by `docs/generate_pdf.py`) |

## Record and replay

`RECORD_FILE=btc.ndjson crypto-orderbook` writes every snapshot and every raw depth event, with the time it arrived, while the service runs. `crypto-orderbook replay btc.ndjson` rebuilds the book offline with the same sequencing rules, so a recording is a reproducible dataset for backtests or for debugging a session afterwards. A real 20-second recording from Binance.US, cut at the update the live service was serving:

```text
$ crypto-orderbook replay btc.ndjson
snapshots 1  events 17  applied 16  skipped 1  gaps 0  bad lines 0
final book at update 3354085117: best bid 84876.93000000 x 0.00330000, best ask 84877.41000000 x 0.00330000, spread 0.48000000, 737 bid / 642 ask levels
```

The live `/book/best` at update 3354085117 showed the same bid, ask and spread.

## Rate limits

The snapshot request checks Binance's answer: a `429` waits as long as `Retry-After` says (Binance bans the IP with `418` if a client keeps going), and the used weight from `X-MBX-USED-WEIGHT-1M` is exported as `orderbook_rest_used_weight_1m`. The service waits for the stream's first event before taking a snapshot, as Binance's procedure says, so a start or reconnect costs one snapshot instead of two.

## Limitations

- One symbol per process.
- Binance spot and USD-M futures (binance.com, or Binance.US for spot via the variables above). Futures were checked against Binance's documented rules with unit tests; they could not be run live from the US, where `fapi.binance.com` answers 451.
- The book is behind a `std::sync::RwLock`; this is a correctness-first design, not a latency-optimized matching engine.

This is market data infrastructure, not a trading system. It places no orders. Research and educational code, not financial advice.
