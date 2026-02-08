#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod config;
mod connection;
mod log_buffer;
mod tray;

slint::include_modules!();

use log_buffer::{LogBuffer, LogBufferLayer};
use single_instance::SingleInstance;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() {
    let instance = SingleInstance::new("remoshot-client").unwrap();
    if !instance.is_single() {
        eprintln!("RemoShot client is already running!");
        std::process::exit(1);
    }

    let log_buf = LogBuffer::new(500);

    let registry = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("remoshot_client=info".parse().unwrap()),
        )
        .with(LogBufferLayer::new(log_buf.clone()));

    #[cfg(debug_assertions)]
    let registry = registry.with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr));

    registry.init();

    tray::run(log_buf);
}
