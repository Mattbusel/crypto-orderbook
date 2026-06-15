use std::sync::{Arc, RwLock};

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::Deserialize;
use tracing::warn;
use tokio::sync::mpsc;

use crate::metrics;
use crate::trades::vwap::VwapWindow;
use crate::ws::WsEvent;

/// The shape Binance sends on the `@trade` stream.
/// Fields named by Binance's convention.
#[derive(Deserialize)]
struct RawTrade {
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    quantity: String,
    /// Trade time in milliseconds since epoch. Used for the VWAP window.
    #[serde(rename = "T")]
    trade_time_ms: u64,
}

/// Consumes the trade WebSocket stream and feeds the rolling VWAP window.
pub struct TradeManager {
    rx:   mpsc::Receiver<WsEvent>,
    vwap: Arc<RwLock<VwapWindow>>,
}

impl TradeManager {
    pub fn new(rx: mpsc::Receiver<WsEvent>, vwap: Arc<RwLock<VwapWindow>>) -> Self {
        Self { rx, vwap }
    }

    pub async fn run(mut self) {
        loop {
            match self.rx.recv().await {
                None => return,
                Some(WsEvent::Reconnected) => continue,
                Some(WsEvent::Message(raw)) => {
                    let trade: RawTrade = match serde_json::from_str(&raw) {
                        Ok(t)  => t,
                        Err(e) => {
                            warn!(error = %e, "failed to parse trade event");
                            continue;
                        }
                    };

                    let price = match trade.price.parse::<Decimal>() {
                        Ok(p)  => p,
                        Err(_) => continue,
                    };
                    let qty = match trade.quantity.parse::<Decimal>() {
                        Ok(q)  => q,
                        Err(_) => continue,
                    };

                    let mut window = self.vwap.write().expect("vwap lock poisoned");
                    window.add(price, qty, trade.trade_time_ms);

                    if let Some(v) = window.vwap() {
                        if let Some(f) = v.to_f64() {
                            metrics::TRADE_VWAP_1M.set(f);
                        }
                    }
                    if let Some(vol) = window.volume().to_f64() {
                        metrics::TRADE_VOLUME_1M.set(vol);
                    }
                }
            }
        }
    }
}
