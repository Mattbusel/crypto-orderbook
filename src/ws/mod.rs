pub mod client;

// The event type that crosses the channel boundary between the WebSocket
// client task and the order book manager task.
//
// This enum is the contract between transport and domain logic.
// The WS client never touches book state; the manager never touches sockets.
#[derive(Debug)]
pub enum WsEvent {
    // A raw JSON frame received from the exchange.
    // Unparsed — parsing is the manager's responsibility.
    Message(String),

    // The WebSocket connection was lost and has been re-established.
    // The manager must discard all book state and re-sync from REST on receipt.
    Reconnected,
}
