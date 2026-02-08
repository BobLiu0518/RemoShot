use chrono::Utc;
use std::sync::Arc;

use crate::state::AppState;

pub async fn cleanup_loop(state: Arc<AppState>) {
    let interval = std::time::Duration::from_secs(60);
    loop {
        tokio::time::sleep(interval).await;

        let retention = chrono::Duration::minutes(state.retention_mins as i64);
        let cutoff = Utc::now() - retention;

        let mut images = state.stored_images.lock().await;
        let mut i = 0;
        while i < images.len() {
            if images[i].created_at < cutoff {
                let img = images.swap_remove(i);
                if let Err(e) = std::fs::remove_file(&img.path) {
                    tracing::warn!("failed to remove expired image {:?}: {}", img.path, e);
                } else {
                    tracing::info!("removed expired image: {:?}", img.path);
                }
            } else {
                i += 1;
            }
        }
    }
}
