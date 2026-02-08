use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    AuthChallenge { nonce: String },
    ScreenshotRequest { request_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    AuthResponse {
        name: String,
        hmac: String,
    },
    ScreenshotResponse {
        request_id: String,
        screenshots: Vec<ScreenshotData>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotData {
    pub monitor: u32,
    pub data: Vec<u8>,
}

pub fn compute_hmac(secret: &str, nonce: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(nonce.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn verify_hmac(secret: &str, nonce: &str, hmac_hex: &str) -> bool {
    let expected = compute_hmac(secret, nonce);
    expected == hmac_hex
}
