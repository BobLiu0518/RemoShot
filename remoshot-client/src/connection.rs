use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use crate::capture;
use remoshot_common::{ClientMessage, ServerMessage};

pub async fn run(
    server_addr: String,
    machine_name: String,
    status_tx: mpsc::UnboundedSender<ConnectionStatus>,
    mut cancel_rx: watch::Receiver<bool>,
) {
    let mut attempt: u32 = 0;

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

                if handle_connection(ws_stream, machine_name.clone(), &status_tx, &mut cancel_rx).await {
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
    status_tx: &mpsc::UnboundedSender<ConnectionStatus>,
    cancel_rx: &mut watch::Receiver<bool>,
) -> bool {
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    let register = ClientMessage::Register {
        name: machine_name.clone(),
    };
    let msg = serde_json::to_string(&register).unwrap();
    if let Err(e) = ws_tx.send(Message::Text(msg.into())).await {
        tracing::error!("failed to send register: {}", e);
        let _ = status_tx.send(ConnectionStatus::Disconnected);
        return false;
    }
    tracing::info!("registered as '{}'", machine_name);

    loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                tracing::info!("connection cancelled");
                return true;
            }
            msg_opt = ws_rx.next() => {
                if !handle_message(msg_opt, &mut ws_tx).await {
                    break;
                }
            }
        }
    }

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
            match serde_json::from_str::<ServerMessage>(&text) {
                Ok(ServerMessage::ScreenshotRequest { request_id }) => {
                    tracing::info!("screenshot request: {}", request_id);

                    let screenshots = tokio::task::spawn_blocking(
                        capture::capture_all_screens,
                    )
                    .await
                    .unwrap_or_default();

                    tracing::info!("captured {} screenshots for request {}", screenshots.len(), request_id);

                    let response = ClientMessage::ScreenshotResponse {
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
            let _ = ws_tx.send(Message::Pong(data)).await;
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
