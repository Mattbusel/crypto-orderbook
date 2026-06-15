use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

use crate::ws::WsEvent;

// Backoff parameters. These are tuned for a public exchange WebSocket:
// - Start at 1s so transient drops recover fast.
// - Double each attempt so we don't hammer a struggling endpoint.
// - Cap at 30s so we recover within a minute after prolonged outages.
// - Add jitter up to 1s to desync simultaneous reconnect storms.
const BACKOFF_BASE_MS: u64 = 1_000;
const BACKOFF_MAX_MS: u64 = 30_000;
const BACKOFF_JITTER_MS: u64 = 1_000;

// Spawns a Tokio task that maintains a persistent WebSocket connection to
// the given URL, forwarding all frames and reconnect signals to `tx`.
//
// This function returns immediately. The task runs until `tx` is dropped
// (receiver closed) or the process exits.
//
// Why spawn instead of returning a Future the caller drives?
// The WS client is a long-lived background actor. Spawning gives it its
// own Tokio task with independent scheduling. If we returned a Future,
// the caller would have to drive it with select! or join!, coupling the
// WS lifecycle to whatever the caller is doing.
pub fn spawn(url: url::Url, tx: mpsc::Sender<WsEvent>) {
    tokio::spawn(run(url, tx));
}

async fn run(url: url::Url, tx: mpsc::Sender<WsEvent>) {
    let mut attempt: u32 = 0;

    loop {
        info!(attempt, %url, "connecting to WebSocket");

        match connect_async(url.as_str()).await {
            Ok((ws_stream, _response)) => {
                info!("WebSocket connected");

                // Reset backoff on successful connection.
                // Only reset AFTER we know the connection succeeded —
                // resetting on the first message would mask repeated
                // connect-then-immediately-drop failures.
                if attempt > 0 {
                    attempt = 0;
                    // Signal the manager that we reconnected and it must resync.
                    // If the receiver is gone, the task has no purpose — exit.
                    if tx.send(WsEvent::Reconnected).await.is_err() {
                        return;
                    }
                }

                // Drive the stream until it closes or errors.
                let closed = drive_stream(ws_stream, &tx).await;

                match closed {
                    DriveResult::ReceiverGone => {
                        // Manager dropped its end of the channel — clean shutdown.
                        info!("WS receiver gone, shutting down client task");
                        return;
                    }
                    DriveResult::StreamClosed => {
                        warn!("WebSocket stream closed, will reconnect");
                    }
                    DriveResult::StreamError(e) => {
                        error!(error = %e, "WebSocket stream error, will reconnect");
                    }
                }
            }
            Err(e) => {
                error!(attempt, error = %e, "WebSocket connection failed");
            }
        }

        // Back off before next attempt.
        let backoff = backoff_duration(attempt);
        warn!(ms = backoff.as_millis(), "backing off before reconnect");
        tokio::time::sleep(backoff).await;
        attempt = attempt.saturating_add(1);
    }
}

// The result of driving a WebSocket stream to completion.
// Named variants force the caller to handle each case explicitly
// rather than collapsing them into a single "something went wrong".
enum DriveResult {
    ReceiverGone,
    StreamClosed,
    StreamError(tokio_tungstenite::tungstenite::Error),
}

// Reads frames from `ws_stream` and forwards text frames to `tx`.
// Returns when the stream ends (cleanly or with an error) or the receiver drops.
//
// Binary frames are ignored — Binance only sends text JSON.
// Ping/Pong frames are handled automatically by tungstenite at the transport
// layer; we don't see them here unless we explicitly ask for raw frames.
// Close frames terminate the loop cleanly via StreamClosed.
async fn drive_stream<S>(
    mut ws_stream: tokio_tungstenite::WebSocketStream<S>,
    tx: &mpsc::Sender<WsEvent>,
) -> DriveResult
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    while let Some(result) = ws_stream.next().await {
        match result {
            Ok(Message::Text(text)) => {
                if tx.send(WsEvent::Message(text.to_string())).await.is_err() {
                    return DriveResult::ReceiverGone;
                }
            }
            Ok(Message::Close(_)) => {
                return DriveResult::StreamClosed;
            }
            Ok(_) => {
                // Ping, Pong, Binary — ignore. tungstenite handles Ping/Pong
                // at the transport layer automatically.
            }
            Err(e) => {
                return DriveResult::StreamError(e);
            }
        }
    }

    // Stream returned None — server closed without a Close frame.
    DriveResult::StreamClosed
}

// Computes the backoff duration for the given attempt number.
// Formula: min(base * 2^attempt, max) + random jitter
//
// Why saturating_pow? On attempt 64+, 2^attempt overflows u64.
// Saturating keeps it at u64::MAX rather than wrapping to 0,
// which would produce a zero backoff after many failures — the worst outcome.
fn backoff_duration(attempt: u32) -> Duration {
    let exp = BACKOFF_BASE_MS.saturating_mul(2u64.saturating_pow(attempt));
    let capped = exp.min(BACKOFF_MAX_MS);
    let jitter = rand_jitter();
    Duration::from_millis(capped + jitter)
}

// Returns a pseudo-random jitter in [0, BACKOFF_JITTER_MS).
// We use the low bits of the current time rather than pulling in a rand crate
// for this single use. It's not cryptographically random — it doesn't need to be.
// We just need clients to desync from each other.
fn rand_jitter() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos as u64) % BACKOFF_JITTER_MS
}
