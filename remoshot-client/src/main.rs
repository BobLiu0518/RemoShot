#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod config;
mod connection;
mod log_buffer;
mod permission;
mod single_instance;
mod tray;

slint::include_modules!();

use log_buffer::{LogBuffer, LogBufferLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        use objc2_foundation::MainThreadMarker;
        let mtm = MainThreadMarker::new().unwrap();
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }

    let _ = match single_instance::SingleInstance::new("remoshot") {
        Ok(inst) => inst,
        Err(e) => {
            eprintln!("RemoShot client is already running: {}", e);
            std::process::exit(1);
        }
    };

    if !permission::check_and_request_screen_recording_permission() {
        eprintln!(
            "no screen recording permission, please grant permission in system preferences and restart"
        );
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
