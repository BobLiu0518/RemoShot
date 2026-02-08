use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, broadcast, oneshot};

pub struct ConnectedClient {
    pub name: String,
    pub tx: tokio::sync::mpsc::UnboundedSender<String>,
}

pub struct PendingRequest {
    pub expected: usize,
    pub received: HashMap<String, Vec<String>>,
    pub notify: Option<oneshot::Sender<HashMap<String, Vec<String>>>>,
}

pub struct StoredImage {
    pub path: PathBuf,
    pub created_at: DateTime<Utc>,
}

pub struct AppState {
    pub clients: RwLock<HashMap<usize, ConnectedClient>>,
    pub next_client_id: Mutex<usize>,
    pub pending_requests: RwLock<HashMap<String, Arc<Mutex<PendingRequest>>>>,
    pub stored_images: Mutex<Vec<StoredImage>>,
    pub retention_mins: u64,
    pub image_dir: PathBuf,
    pub secret_key: String,
    pub _shutdown_tx: broadcast::Sender<()>,
}

impl AppState {
    pub fn new(retention_mins: u64, image_dir: PathBuf, secret_key: String) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            clients: RwLock::new(HashMap::new()),
            next_client_id: Mutex::new(0),
            pending_requests: RwLock::new(HashMap::new()),
            stored_images: Mutex::new(Vec::new()),
            retention_mins,
            image_dir,
            secret_key,
            _shutdown_tx: shutdown_tx,
        }
    }

    pub async fn next_id(&self) -> usize {
        let mut id = self.next_client_id.lock().await;
        let current = *id;
        *id += 1;
        current
    }

    pub async fn register_client(
        &self,
        id: usize,
        name: String,
        tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        let mut clients = self.clients.write().await;
        clients.insert(id, ConnectedClient { name, tx });
    }

    pub async fn unregister_client(&self, id: usize) {
        let mut clients = self.clients.write().await;
        clients.remove(&id);
    }

    pub async fn broadcast_screenshot_request(&self, request_id: &str) -> usize {
        let clients = self.clients.read().await;
        let msg = serde_json::to_string(&remoshot_common::ServerMessage::ScreenshotRequest {
            request_id: request_id.to_string(),
        })
        .unwrap();

        let mut count = 0;
        for client in clients.values() {
            if client.tx.send(msg.clone()).is_ok() {
                count += 1;
            }
        }
        count
    }

    pub async fn store_image(&self, path: PathBuf) {
        let mut images = self.stored_images.lock().await;
        images.push(StoredImage {
            path,
            created_at: Utc::now(),
        });
    }
}
