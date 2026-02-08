use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::state::AppState;
use remoshot_common::ClientMessage;

pub async fn run_ws_server(addr: SocketAddr, state: Arc<AppState>) {
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    let client_id = state.next_id().await;

    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let client_name = loop {
        match ws_rx.next().await {
            Some(Ok(Message::Text(text))) => {
                if let Ok(ClientMessage::Register { name }) = serde_json::from_str(&text) {
                    break name;
                }
                tracing::warn!("expected Register message, got: {}", text);
            }
            Some(Ok(Message::Close(_))) | None => {
                send_task.abort();
                return;
            }
            _ => continue,
        }
    };

    tracing::info!("client registered: {} (id={})", client_name, client_id);
    state
        .register_client(client_id, client_name.clone(), tx)
        .await;

    while let Some(msg_result) = ws_rx.next().await {
        match msg_result {
            Ok(Message::Text(text)) => match serde_json::from_str::<ClientMessage>(&text) {
                Ok(ClientMessage::Register { .. }) => {
                    tracing::warn!("duplicate register from {}", client_name);
                }
                Ok(ClientMessage::ScreenshotResponse { .. }) => {
                    tracing::warn!("unexpected JSON screenshot response from {}", client_name);
                }
                Err(e) => {
                    tracing::warn!("invalid JSON message from {}: {}", client_name, e);
                }
            },
            Ok(Message::Binary(data)) => match rmp_serde::from_slice::<ClientMessage>(&data) {
                Ok(ClientMessage::ScreenshotResponse {
                    request_id,
                    screenshots,
                }) => {
                    handle_screenshot_response(&state, &client_name, &request_id, screenshots)
                        .await;
                }
                Ok(ClientMessage::Register { .. }) => {
                    tracing::warn!("unexpected MessagePack register from {}", client_name);
                }
                Err(e) => {
                    tracing::warn!("invalid MessagePack message from {}: {}", client_name, e);
                }
            },
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }

    tracing::info!("client disconnected: {} (id={})", client_name, client_id);
    state.unregister_client(client_id).await;
    send_task.abort();
}

async fn handle_screenshot_response(
    state: &Arc<AppState>,
    client_name: &str,
    request_id: &str,
    screenshots: Vec<remoshot_common::ScreenshotData>,
) {
    tracing::info!(
        "received screenshot response from {} for request {}: {} images",
        client_name,
        request_id,
        screenshots.len()
    );

    let mut image_paths = Vec::new();

    for shot in &screenshots {
        let filename = format!(
            "{}_{}_{}_{}.jpg",
            request_id,
            client_name,
            shot.monitor,
            chrono::Utc::now().timestamp_millis()
        );
        let path = state.image_dir.join(&filename);

        if let Err(e) = tokio::fs::write(&path, &shot.data).await {
            tracing::warn!("failed to write image {}: {}", filename, e);
            continue;
        }

        let url_path = format!("/images/{}", filename);
        image_paths.push(url_path);
        state.store_image(path).await;
    }

    let pending = {
        let requests = state.pending_requests.read().await;
        requests.get(request_id).cloned()
    };

    if let Some(pending) = pending {
        let mut req = pending.lock().await;
        req.received.insert(client_name.to_string(), image_paths);

        if req.received.len() >= req.expected {
            if let Some(notify) = req.notify.take() {
                let _ = notify.send(req.received.clone());
            }
        }
    }
}
