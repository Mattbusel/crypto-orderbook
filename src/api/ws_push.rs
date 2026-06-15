use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::IntoResponse,
};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::api::routes::AppState;
use crate::orderbook::book::PriceLevel;

/// A compact top-of-book snapshot broadcast to every WebSocket subscriber.
/// We only push what changes most: the best bid/ask and spread.
/// Clients that need depth can call /book/snapshot on demand.
#[derive(Serialize, Clone)]
pub struct BookPush {
    pub symbol:         String,
    pub best_bid_price: Option<String>,
    pub best_ask_price: Option<String>,
    pub spread:         Option<String>,
    pub micro_price:    Option<String>,
    pub last_update_id: u64,
}

/// A broadcast channel sender that carries serialized BookPush JSON strings.
///
/// broadcast::channel is many-producer, many-consumer.
/// The Manager holds a Sender and calls .send() after each book update.
/// Each connected WebSocket client holds its own Receiver (created via .subscribe()).
///
/// When a client is too slow to consume messages, it gets a Lagged error and
/// skips the missed snapshots - acceptable for a display client, because the next
/// message will contain the current state. We do not buffer indefinitely.
pub type PushSender = broadcast::Sender<String>;

/// Called when a client connects to GET /ws/book.
/// axum upgrades the HTTP connection to WebSocket and hands us the socket.
pub async fn ws_book_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let rx = state.push_tx.subscribe();
    ws.on_upgrade(move |socket| drive_client(socket, rx))
}

/// Drive a single WebSocket client for the lifetime of its connection.
///
/// Reads from the broadcast receiver and forwards each message to the client.
/// Returns (and drops the socket) when:
///   - the client disconnects (send error)
///   - the broadcast channel is closed (process is shutting down)
async fn drive_client(mut socket: WebSocket, mut rx: broadcast::Receiver<String>) {
    loop {
        match rx.recv().await {
            Ok(payload) => {
                if socket.send(Message::Text(payload)).await.is_err() {
                    // Client disconnected - clean exit, not an error.
                    return;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                // This client is too slow. It missed n messages.
                // We skip them - the next message will have the current state.
                // Alternatively, we could send a warning payload here.
                tracing::debug!(skipped = n, "WS client lagged, skipping messages");
            }
            Err(broadcast::error::RecvError::Closed) => {
                // The broadcast sender was dropped. The system is shutting down.
                return;
            }
        }
    }
}

/// Serialize a BookPush to JSON, returning None if serialization fails.
/// Called by the Manager after each successful book update.
pub fn serialize_push(
    symbol: &str,
    bid: Option<PriceLevel>,
    ask: Option<PriceLevel>,
    spread: Option<rust_decimal::Decimal>,
    micro_price: Option<rust_decimal::Decimal>,
    last_update_id: u64,
) -> Option<String> {
    let push = BookPush {
        symbol:         symbol.to_owned(),
        best_bid_price: bid.map(|l| l.price.to_string()),
        best_ask_price: ask.map(|l| l.price.to_string()),
        spread:         spread.map(|s| s.to_string()),
        micro_price:    micro_price.map(|m| m.to_string()),
        last_update_id,
    };
    serde_json::to_string(&push).ok()
}
