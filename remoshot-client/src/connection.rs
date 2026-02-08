use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use crate::capture;

pub async fn run(
    server_addr: String,
    machine_name: String,
    secret_key: String,
    status_tx: mpsc::UnboundedSender<ConnectionStatus>,
    mut cancel_rx: watch::Receiver<bool>,
) {
    let mut attempt: u32 = 0;
    let _ = rustls::crypto::ring::default_provider().install_default();

    loop {
        if *cancel_rx.borrow() {
            return;
        }

        let _ = status_tx.send(ConnectionStatus::Connecting);
        tracing::info!("connecting to {}...", server_addr);

        match tokio_tungstenite::connect_async(&server_addr).await {
            Ok((ws_stream, _)) => {
                attempt = 0;
                let _ = status_tx.send(ConnectionStatus::Connected);
                tracing::info!("connected to {}", server_addr);

                if handle_connection(
                    ws_stream,
                    machine_name.clone(),
                    secret_key.clone(),
                    &status_tx,
                    &mut cancel_rx,
                )
                .await
                {
                    return;
                }
            }
            Err(e) => {
                tracing::error!("connection failed: {}", e);
                let _ = status_tx.send(ConnectionStatus::Disconnected);
            }
        }

        let base_delay = std::cmp::min(2u64.saturating_pow(attempt), 60);
        let jitter = rand::random::<f64>() * base_delay as f64 * 0.3;
        let delay = std::time::Duration::from_secs_f64(base_delay as f64 + jitter);
        tracing::info!("reconnecting in {:.1}s...", delay.as_secs_f64());

        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = cancel_rx.changed() => {
                return;
            }
        }
        attempt += 1;
    }
}

async fn handle_connection(
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    machine_name: String,
    secret_key: String,
    status_tx: &mpsc::UnboundedSender<ConnectionStatus>,
    cancel_rx: &mut watch::Receiver<bool>,
) -> bool {
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    let nonce = loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                tracing::info!("connection cancelled during auth");
                return true;
            }
            msg_opt = ws_rx.next() => {
                match msg_opt {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<remoshot_common::ServerMessage>(&text) {
                            Ok(remoshot_common::ServerMessage::AuthChallenge { nonce }) => {
                                break nonce;
                            }
                            Ok(msg) => {
                                tracing::warn!("unexpected message during auth: {:?}", msg);
                                continue;
                            }
                            Err(e) => {
                                tracing::warn!("invalid auth message: {}", e);
                                continue;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        tracing::info!("server closed connection during auth");
                        return false;
                    }
                    Some(Err(e)) => {
                        tracing::error!("ws error during auth: {}", e);
                        return false;
                    }
                    None => return false,
                    _ => continue,
                }
            }
        }
    };

    let hmac = remoshot_common::compute_hmac(&secret_key, &nonce);
    let auth_response = remoshot_common::ClientMessage::AuthResponse {
        name: machine_name.clone(),
        hmac,
    };
    let msg = serde_json::to_string(&auth_response).unwrap();
    if let Err(e) = ws_tx.send(Message::Text(msg.into())).await {
        tracing::error!("failed to send auth response: {}", e);
        let _ = status_tx.send(ConnectionStatus::Disconnected);
        return false;
    }
    tracing::info!("authenticated as '{}'", machine_name);

    let (ping_tx, mut ping_rx) = mpsc::unbounded_channel();

    let ping_task = {
        let ping_tx = ping_tx.clone();
        tokio::spawn(async move {
            let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(30));
            ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ping_interval.tick().await;
                if ping_tx.send(()).is_err() {
                    break;
                }
            }
        })
    };

    loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                tracing::info!("connection cancelled");
                ping_task.abort();
                return true;
            }
            _ = ping_rx.recv() => {
                if ws_tx.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
            msg_opt = ws_rx.next() => {
                if !handle_message(msg_opt, &mut ws_tx).await {
                    break;
                }
            }
        }
    }

    ping_task.abort();

    let _ = status_tx.send(ConnectionStatus::Disconnected);
    false
}

async fn handle_message(
    msg_opt: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
    ws_tx: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
) -> bool {
    match msg_opt {
        Some(Ok(Message::Text(text))) => {
            match serde_json::from_str::<remoshot_common::ServerMessage>(&text) {
                Ok(remoshot_common::ServerMessage::ScreenshotRequest { request_id }) => {
                    tracing::info!("screenshot request: {}", request_id);

                    let screenshots = tokio::task::spawn_blocking(capture::capture_all_screens)
                        .await
                        .unwrap_or_default();

                    tracing::info!(
                        "captured {} screenshots for request {}",
                        screenshots.len(),
                        request_id
                    );

                    let response = remoshot_common::ClientMessage::ScreenshotResponse {
                        request_id: request_id.clone(),
                        screenshots,
                    };
                    let msg = rmp_serde::to_vec(&response).unwrap();
                    if let Err(e) = ws_tx.send(Message::Binary(msg.into())).await {
                        tracing::error!("failed to send response: {}", e);
                        return false;
                    }
                    tracing::info!("screenshot response sent for request {}", request_id);
                }
                Ok(remoshot_common::ServerMessage::AuthChallenge { .. }) => {
                    tracing::warn!("unexpected auth challenge after authentication");
                }
                Err(e) => {
                    tracing::warn!("unknown message: {}", e);
                }
            }
            true
        }
        Some(Ok(Message::Close(_))) => {
            tracing::info!("server closed connection");
            false
        }
        Some(Ok(Message::Ping(data))) => {
            let _ = ws_tx.send(Message::Pong(data.clone())).await;
            true
        }
        Some(Err(e)) => {
            tracing::error!("ws error: {}", e);
            false
        }
        None => false,
        _ => true,
    }
}

#[derive(Debug, Clone)]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
}
