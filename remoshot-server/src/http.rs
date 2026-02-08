use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::{ConnectInfo, State};
use axum::response::Json;
use axum::routing::get;
use tokio::sync::{Mutex, oneshot};
use tower_http::services::ServeDir;

use crate::state::{AppState, PendingRequest};

pub async fn run_http_server(addr: SocketAddr, state: Arc<AppState>, image_dir: PathBuf) {
    let app = Router::new()
        .route("/screenshot", get(screenshot_handler))
        .nest_service("/images", ServeDir::new(image_dir))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

async fn screenshot_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Json<HashMap<String, Vec<String>>> {
    let request_id = uuid::Uuid::new_v4().to_string();
    tracing::info!(
        "screenshot request from {}, request_id: {}",
        addr,
        request_id
    );

    let (tx, rx) = oneshot::channel();

    let expected = state.broadcast_screenshot_request(&request_id).await;
    tracing::info!(
        "broadcasted screenshot request {} to {} clients",
        request_id,
        expected
    );

    if expected == 0 {
        tracing::warn!("no clients available for screenshot request {}", request_id);
        return Json(HashMap::new());
    }

    let pending = Arc::new(Mutex::new(PendingRequest {
        expected,
        received: HashMap::new(),
        notify: Some(tx),
    }));

    {
        let mut requests = state.pending_requests.write().await;
        requests.insert(request_id.clone(), pending.clone());
    }

    let result = match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
        Ok(Ok(map)) => {
            tracing::info!("received all expected responses for request {}", request_id);
            map
        }
        _ => {
            tracing::warn!("timeout or partial responses for request {}", request_id);
            let req = pending.lock().await;
            req.received.clone()
        }
    };

    let mut final_result = result;
    {
        let clients = state.clients.read().await;
        for client in clients.values() {
            final_result
                .entry(client.name.clone())
                .or_insert_with(Vec::new);
        }
    }

    {
        let mut requests = state.pending_requests.write().await;
        requests.remove(&request_id);
    }

    Json(final_result)
}
