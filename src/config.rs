/// All runtime configuration for the engine.
/// Values are read from environment variables with sensible defaults.
/// This is the single place where configuration enters the system.
#[derive(Debug, Clone)]
pub struct Config {
    /// Trading pair to track, e.g. "BTCUSDT"
    pub symbol: String,

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
        let symbol_lower = symbol.to_lowercase();

        Self {
            ws_base: std::env::var("WS_BASE")
                .unwrap_or_else(|_| format!("wss://stream.binance.com:9443/ws/{symbol_lower}@depth")),
            rest_base: std::env::var("REST_BASE")
                .unwrap_or_else(|_| "https://api.binance.com".into()),
            api_port: std::env::var("API_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000),
            channel_buffer: std::env::var("CHANNEL_BUFFER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10_000),
            symbol,
        }
    }

    pub fn ws_url(&self) -> url::Url {
        self.ws_base.parse().expect("invalid WS_BASE URL")
    }

    pub fn trade_ws_url(&self) -> url::Url {
        let symbol_lower = self.symbol.to_lowercase();
        std::env::var("TRADE_WS_BASE")
            .unwrap_or_else(|_| format!("wss://stream.binance.com:9443/ws/{symbol_lower}@trade"))
            .parse()
            .expect("invalid trade WS URL")
    }
}
