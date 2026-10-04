/// All runtime configuration for the engine.
/// Values are read from environment variables with sensible defaults.
/// This is the single place where configuration enters the system.
#[derive(Debug, Clone)]
pub struct Config {
    /// Trading pair to track, e.g. "BTCUSDT"
    pub symbol: String,

    /// Which Binance market: spot (default) or USD-M futures (`MARKET=usdm`).
    pub market: crate::orderbook::sync::Market,

    /// Levels requested in the REST snapshot (`DEPTH_LIMIT`). Binance charges
    /// request weight by size: on spot 5000 levels cost 250 of the 6000
    /// weight a minute allows, 1000 levels cost 50.
    pub depth_limit: u32,

    /// Binance WebSocket stream base URL
    pub ws_base: String,

    /// Binance REST API base URL
    pub rest_base: String,

    /// Port to bind the HTTP API on
    pub api_port: u16,

    /// How many WebSocket events the mpsc channel can buffer before
    /// the sender starts to slow down. Large enough to absorb the
    /// snapshot fetch delay without dropping events.
    pub channel_buffer: usize,
}

impl Config {
    pub fn from_env() -> Self {
        let symbol = std::env::var("SYMBOL").unwrap_or_else(|_| "BTCUSDT".into());
        let market = std::env::var("MARKET")
            .ok()
            .and_then(|m| crate::orderbook::sync::Market::parse(&m))
            .unwrap_or(crate::orderbook::sync::Market::Spot);
        let depth_limit = std::env::var("DEPTH_LIMIT")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(market.max_depth_limit())
            .clamp(5, market.max_depth_limit());

        Self {
            ws_base: std::env::var("WS_BASE").unwrap_or_else(|_| market.default_ws_url(&symbol)),
            rest_base: std::env::var("REST_BASE")
                .unwrap_or_else(|_| market.default_rest_base().into()),
            api_port: std::env::var("API_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000),
            channel_buffer: std::env::var("CHANNEL_BUFFER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10_000),
            symbol,
            market,
            depth_limit,
        }
    }

    pub fn ws_url(&self) -> url::Url {
        self.ws_base.parse().expect("invalid WS_BASE URL")
    }

    pub fn trade_ws_url(&self) -> url::Url {
        std::env::var("TRADE_WS_BASE")
            .unwrap_or_else(|_| self.market.default_trade_ws_url(&self.symbol))
            .parse()
            .expect("invalid trade WS URL")
    }
}
