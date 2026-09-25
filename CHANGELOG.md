# Changelog

## 0.2.0 (2026-09-25)

- Prebuilt binaries for Windows, macOS (Apple Silicon and Intel) and Linux on every GitHub Release, with SHA256SUMS.txt.
- `--help` and `--version` flags; logs default to `info` so a plain run shows what it is doing.
- `REST_BASE` is now honoured by the snapshot request, so the service works against Binance.US where binance.com returns 451.
- CI workflow (build and test).
- Crate metadata for crates.io (`cargo install crypto-orderbook`).

## 0.1.0

- Initial release: Binance snapshot-plus-diff order book, REST, WebSocket push and Prometheus metrics.
