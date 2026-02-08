use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server_addr: String,
    pub machine_name: String,
    pub secret_key: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_addr: "ws://127.0.0.1:8283/ws".to_string(),
            machine_name: whoami().unwrap_or_else(|| "unknown".to_string()),
            secret_key: String::new(),
        }
    }
}

fn whoami() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
}

fn config_path() -> PathBuf {
    let proj = ProjectDirs::from("", "", "RemoShot").expect("cannot determine config directory");
    let dir = proj.config_dir();
    fs::create_dir_all(dir).ok();
    dir.join("config.json")
}

pub fn load() -> Option<Config> {
    let path = config_path();
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save(config: &Config) {
    let path = config_path();
    let data = serde_json::to_string_pretty(config).unwrap();
    fs::write(path, data).ok();
}
