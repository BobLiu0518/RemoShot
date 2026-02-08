mod cleanup;
mod http;
mod secret;
mod state;
mod ws;

use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "remoshot-server", about = "RemoShot screenshot server")]
struct Args {
    #[arg(long)]
    ws_port: Option<u16>,

    #[arg(long)]
    http_addr: Option<String>,

    #[arg(long)]
    retention: Option<u64>,
}

fn prompt(msg: &str) -> String {
    dialoguer::Input::<String>::new()
        .with_prompt(msg)
        .interact_text()
        .unwrap()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("remoshot_server=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    let secret_key = secret::load_or_generate_secret();
    tracing::info!("Server SecretKey: {}", secret_key);

    let ws_port: u16 = args.ws_port.unwrap_or_else(|| {
        prompt("WebSocket port for client connections")
            .parse()
            .expect("invalid port")
    });

    let http_addr_str = args
        .http_addr
        .unwrap_or_else(|| prompt("HTTP listen address (e.g. 127.0.0.1:8113)"));
    let http_addr: SocketAddr = http_addr_str.parse().expect("invalid HTTP address");

    let retention_mins: u64 = args.retention.unwrap_or_else(|| {
        prompt("Screenshot retention time in minutes")
            .parse()
            .expect("invalid retention time")
    });

    let image_dir = PathBuf::from("images");
    std::fs::create_dir_all(&image_dir).expect("failed to create images directory");

    let state = Arc::new(state::AppState::new(
        retention_mins,
        image_dir.clone(),
        secret_key,
    ));

    let cleanup_state = state.clone();
    tokio::spawn(async move {
        cleanup::cleanup_loop(cleanup_state).await;
    });

    let ws_state = state.clone();
    let ws_addr: SocketAddr = format!("0.0.0.0:{ws_port}").parse().unwrap();
    tokio::spawn(async move {
        ws::run_ws_server(ws_addr, ws_state).await;
    });

    tracing::info!("WebSocket server listening on {}", ws_addr);
    tracing::info!("HTTP server listening on {}", http_addr);

    http::run_http_server(http_addr, state, image_dir).await;
}
