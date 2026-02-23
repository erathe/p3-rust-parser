use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use tokio::select;
use tracing::{info, warn};

use super::state::AppState;

/// WebSocket upgrade handler — each connected client receives P3 messages and race events as JSON.
pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    info!("WebSocket client connected");

    // Send current race state snapshot to newly connected client
    {
        let engine = state.engine.lock().await;
        let snapshot = engine.state_snapshot();
        if let Ok(json) = serde_json::to_string(&snapshot) {
            let _ = socket.send(WsMessage::text(json)).await;
        }
    }

    let mut p3_rx = state.message_tx.subscribe();
    let mut race_rx = state.race_event_tx.subscribe();

    loop {
        select! {
            result = p3_rx.recv() => {
                match result {
                    Ok(message) => {
                        let json = match serde_json::to_string(message.as_ref()) {
                            Ok(j) => j,
                            Err(e) => {
                                warn!(error = %e, "Failed to serialize P3 message");
                                continue;
                            }
                        };
                        if socket.send(WsMessage::text(json)).await.is_err() {
                            info!("WebSocket client disconnected");
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "WebSocket client lagging on P3 messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("P3 broadcast channel closed");
                        break;
                    }
                }
            }
            result = race_rx.recv() => {
                match result {
                    Ok(event) => {
                        let json = match serde_json::to_string(event.as_ref()) {
                            Ok(j) => j,
                            Err(e) => {
                                warn!(error = %e, "Failed to serialize race event");
                                continue;
                            }
                        };
                        if socket.send(WsMessage::text(json)).await.is_err() {
                            info!("WebSocket client disconnected");
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "WebSocket client lagging on race events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Race event broadcast channel closed");
                        break;
                    }
                }
            }
        }
    }
}
