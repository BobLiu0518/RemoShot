use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::AppState;

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
    let client_id = state.next_id().await;

    let nonce = Uuid::new_v4().to_string();
    let challenge = remoshot_common::ServerMessage::AuthChallenge {
        nonce: nonce.clone(),
    };
    let challenge_msg = serde_json::to_string(&challenge).unwrap();
    if ws_tx
        .send(Message::Text(challenge_msg.into()))
        .await
        .is_err()
    {
        return;
    }

    let client_name = loop {
        match ws_rx.next().await {
            Some(Ok(Message::Text(text))) => {
                if let Ok(remoshot_common::ClientMessage::AuthResponse { name, hmac }) =
                    serde_json::from_str(&text)
                {
                    if remoshot_common::verify_hmac(&state.secret_key, &nonce, &hmac) {
                        break name;
                    } else {
                        tracing::warn!("invalid HMAC from client {}", name);
                        return;
                    }
                }
                tracing::warn!("expected AuthResponse message, got: {}", text);
            }
            Some(Ok(Message::Close(_))) | None => {
                return;
            }
            _ => continue,
        }
    };

    tracing::info!("client authenticated: {} (id={})", client_name, client_id);

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let (pong_tx, mut pong_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    state
        .register_client(client_id, client_name.clone(), tx)
        .await;

    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg_opt = rx.recv() => {
                    match msg_opt {
                        Some(msg) => {
                            if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                pong_data_opt = pong_rx.recv() => {
                    match pong_data_opt {
                        Some(data) => {
                            if ws_tx.send(Message::Pong(data.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    });

    while let Some(msg_result) = ws_rx.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<remoshot_common::ClientMessage>(&text) {
                    Ok(remoshot_common::ClientMessage::AuthResponse { .. }) => {
                        tracing::warn!("duplicate auth from {}", client_name);
                    }
                    Ok(remoshot_common::ClientMessage::ScreenshotResponse { .. }) => {
                        tracing::warn!("unexpected JSON screenshot response from {}", client_name);
                    }
                    Err(e) => {
                        tracing::warn!("invalid JSON message from {}: {}", client_name, e);
                    }
                }
            }
            Ok(Message::Binary(data)) => {
                match rmp_serde::from_slice::<remoshot_common::ClientMessage>(&data) {
                    Ok(remoshot_common::ClientMessage::ScreenshotResponse {
                        request_id,
                        screenshots,
                    }) => {
                        handle_screenshot_response(&state, &client_name, &request_id, screenshots)
                            .await;
                    }
                    Ok(remoshot_common::ClientMessage::AuthResponse { .. }) => {
                        tracing::warn!("unexpected MessagePack auth from {}", client_name);
                    }
                    Err(e) => {
                        tracing::warn!("invalid MessagePack message from {}: {}", client_name, e);
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = pong_tx.send(data.to_vec());
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) | Err(_) => break,
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

        if req.received.len() >= req.expected
            && let Some(notify) = req.notify.take()
        {
            let _ = notify.send(req.received.clone());
        }
    }
}
