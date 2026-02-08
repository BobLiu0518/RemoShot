use rand::Rng;
use std::fs;
use std::path::PathBuf;

fn secret_key_path() -> PathBuf {
    PathBuf::from("secret.key")
}

pub fn load_or_generate_secret() -> String {
    let path = secret_key_path();

    if let Ok(secret) = fs::read_to_string(&path) {
        let secret = secret.trim();
        if !secret.is_empty() {
            return secret.to_string();
        }
    }

    let secret = generate_secret();
    if let Err(e) = fs::write(&path, &secret) {
        tracing::warn!("failed to save secret key: {}", e);
    }

    secret
}

fn generate_secret() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.r#gen();
    hex::encode(bytes)
}
