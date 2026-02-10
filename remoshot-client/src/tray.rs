use slint::ComponentHandle;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tray_icon::TrayIconBuilder;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};

use crate::config::{self, Config};
use crate::connection::{self, ConnectionStatus};
use crate::log_buffer::LogBuffer;
use crate::{LogWindow, SettingsWindow};

pub fn run(log_buf: LogBuffer) {
    let first_launch = config::load().is_none();
    let config = Arc::new(Mutex::new(config::load().unwrap_or_default()));

    let auto_launch = create_auto_launch();
    let is_auto_launch_enabled = auto_launch.is_enabled().unwrap_or(false);

    let menu = Menu::new();
    let item_status = MenuItem::new("Status: Not connected", false, None);
    let item_settings = MenuItem::new("Settings", true, None);
    let item_logs = MenuItem::new("View Logs", true, None);
    let item_auto_launch =
        CheckMenuItem::new("Launch on startup", true, is_auto_launch_enabled, None);
    let item_quit = MenuItem::new("Quit", true, None);
    menu.append(&item_status).unwrap();
    menu.append(&item_settings).unwrap();
    menu.append(&item_logs).unwrap();
    menu.append(&item_auto_launch).unwrap();
    menu.append(&item_quit).unwrap();

    let icon = load_icon();
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("RemoShot")
        .with_icon(icon)
        .with_menu_on_left_click(true)
        .build()
        .expect("failed to create tray icon");

    let cancel_token: Arc<Mutex<Option<tokio::sync::watch::Sender<bool>>>> =
        Arc::new(Mutex::new(None));
    let status_text: Arc<Mutex<String>> = Arc::new(Mutex::new("Not connected".into()));

    if first_launch {
        show_settings_sync(&config);
    }

    let rt = Arc::new(tokio::runtime::Runtime::new().unwrap());
    start_connection(&rt, &config, &cancel_token, &status_text, &item_status);

    let settings_id = item_settings.id().clone();
    let quit_id = item_quit.id().clone();
    let logs_id = item_logs.id().clone();
    let auto_launch_id = item_auto_launch.id().clone();
    let menu_rx = MenuEvent::receiver();

    let config_c = config.clone();
    let cancel_c = cancel_token.clone();
    let status_c = status_text.clone();
    let rt_c = rt.clone();
    let log_buf_c = log_buf.clone();

    let timer = slint::Timer::default();
    let tray_handle = tray;
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            let _ = &tray_handle;

            {
                let text = status_c.lock().unwrap().clone();
                item_status.set_text(format!("Status: {text}"));
            }

            while let Ok(event) = menu_rx.try_recv() {
                if event.id() == &settings_id {
                    show_settings_window(&config_c, &rt_c, &cancel_c, &status_c, &item_status);
                } else if event.id() == &logs_id {
                    show_log_window(&log_buf_c);
                } else if event.id() == &auto_launch_id {
                    handle_auto_launch_toggle(&item_auto_launch);
                } else if event.id() == &quit_id {
                    if let Some(tx) = cancel_c.lock().unwrap().take() {
                        let _ = tx.send(true);
                    }
                    slint::quit_event_loop().ok();
                }
            }
        },
    );

    slint::run_event_loop_until_quit().unwrap();
}

fn show_settings_sync(config: &Arc<Mutex<Config>>) {
    let current = config.lock().unwrap().clone();
    let win = SettingsWindow::new().unwrap();
    win.set_server_addr(current.server_addr.as_str().into());
    win.set_machine_name(current.machine_name.as_str().into());
    win.set_secret_key(current.secret_key.as_str().into());

    let win_weak = win.as_weak();
    let cfg = config.clone();
    win.on_save(move || {
        let w = win_weak.unwrap();
        let new_config = Config {
            server_addr: w.get_server_addr().to_string(),
            machine_name: w.get_machine_name().to_string(),
            secret_key: w.get_secret_key().to_string(),
        };
        config::save(&new_config);
        *cfg.lock().unwrap() = new_config;
        w.hide().ok();
    });

    let win_weak2 = win.as_weak();
    win.on_cancel(move || {
        win_weak2.unwrap().hide().ok();
    });

    let win_weak3 = win.as_weak();
    win.window().on_close_requested(move || {
        if let Some(w) = win_weak3.upgrade() {
            w.hide().ok();
        }
        slint::CloseRequestResponse::HideWindow
    });

    win.run().ok();
}

fn show_settings_window(
    config: &Arc<Mutex<Config>>,
    rt: &Arc<tokio::runtime::Runtime>,
    cancel_token: &Arc<Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
    status_text: &Arc<Mutex<String>>,
    item_status: &MenuItem,
) {
    let current = config.lock().unwrap().clone();
    let win = SettingsWindow::new().unwrap();
    win.set_server_addr(current.server_addr.as_str().into());
    win.set_machine_name(current.machine_name.as_str().into());
    win.set_secret_key(current.secret_key.as_str().into());

    let win_weak = win.as_weak();
    let cfg = config.clone();
    let rt_c = rt.clone();
    let cancel_c = cancel_token.clone();
    let status_c = status_text.clone();
    let item_status_c = item_status.clone();
    win.on_save(move || {
        let w = win_weak.unwrap();
        let new_config = Config {
            server_addr: w.get_server_addr().to_string(),
            machine_name: w.get_machine_name().to_string(),
            secret_key: w.get_secret_key().to_string(),
        };
        config::save(&new_config);
        *cfg.lock().unwrap() = new_config;
        w.hide().ok();

        if let Some(tx) = cancel_c.lock().unwrap().take() {
            let _ = tx.send(true);
        }
        start_connection(&rt_c, &cfg, &cancel_c, &status_c, &item_status_c);
    });

    let win_weak2 = win.as_weak();
    win.on_cancel(move || {
        win_weak2.unwrap().hide().ok();
    });

    let win_weak3 = win.as_weak();
    win.window().on_close_requested(move || {
        if let Some(w) = win_weak3.upgrade() {
            w.hide().ok();
        }
        slint::CloseRequestResponse::HideWindow
    });

    win.show().ok();
}

fn show_log_window(log_buf: &LogBuffer) {
    let win = LogWindow::new().unwrap();
    win.set_log_content(slint::SharedString::from(log_buf.snapshot()));

    let log_buf_c = log_buf.clone();
    let win_weak = win.as_weak();
    log_buf_c.clone().set_update_callback(move || {
        if let Some(w) = win_weak.upgrade() {
            let content = log_buf_c.snapshot();
            w.set_log_content(slint::SharedString::from(content));
        }
    });

    let log_buf_c2 = log_buf.clone();
    let win_weak2 = win.as_weak();
    win.on_clear_logs(move || {
        log_buf_c2.clear();
        if let Some(w) = win_weak2.upgrade() {
            w.set_log_content(slint::SharedString::default());
        }
    });

    let win_weak3 = win.as_weak();
    win.window().on_close_requested(move || {
        if let Some(w) = win_weak3.upgrade() {
            w.hide().ok();
        }
        slint::CloseRequestResponse::HideWindow
    });

    win.show().ok();
}

fn start_connection(
    rt: &Arc<tokio::runtime::Runtime>,
    config: &Arc<Mutex<Config>>,
    cancel_token: &Arc<Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
    status_text: &Arc<Mutex<String>>,
    _item_status: &MenuItem,
) {
    if let Some(tx) = cancel_token.lock().unwrap().take() {
        let _ = tx.send(true);
    }

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    *cancel_token.lock().unwrap() = Some(cancel_tx);

    let cfg = config.lock().unwrap().clone();
    let status = status_text.clone();

    rt.spawn(async move {
        let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel::<ConnectionStatus>();

        let status_c = status.clone();
        tokio::spawn(async move {
            while let Some(s) = status_rx.recv().await {
                let text = match s {
                    ConnectionStatus::Connecting => "Connecting...",
                    ConnectionStatus::Connected => "Connected",
                    ConnectionStatus::Disconnected => "Disconnected",
                };
                *status_c.lock().unwrap() = text.to_string();
            }
        });

        connection::run(
            cfg.server_addr,
            cfg.machine_name,
            cfg.secret_key,
            status_tx,
            cancel_rx,
        )
        .await;
    });
}

fn load_icon() -> tray_icon::Icon {
    let size = 16u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for _ in 0..size * size {
        rgba.extend_from_slice(&[0x33, 0x99, 0xFF, 0xFF]);
    }
    tray_icon::Icon::from_rgba(rgba, size, size).expect("failed to create icon")
}

fn create_auto_launch() -> auto_launch::AutoLaunch {
    let app_path = get_app_path().expect("Failed to get app path");

    auto_launch::AutoLaunchBuilder::new()
        .set_app_name("RemoShot")
        .set_app_path(&app_path.to_string_lossy())
        .build()
        .expect("Failed to create auto-launch")
}

#[cfg(target_os = "macos")]
fn get_app_path() -> Option<PathBuf> {
    use objc2_foundation::NSBundle;

    unsafe {
        let bundle = NSBundle::mainBundle();

        if bundle.bundleIdentifier().is_none() {
            return std::env::current_exe().ok();
        }

        let path = bundle.bundlePath();
        Some(PathBuf::from(path.to_string()))
    }
}

#[cfg(not(target_os = "macos"))]
fn get_app_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

fn handle_auto_launch_toggle(item: &CheckMenuItem) {
    let auto_launch = create_auto_launch();
    let is_enabled = auto_launch.is_enabled().unwrap_or(false);

    match is_enabled {
        true => {
            if let Err(e) = auto_launch.disable() {
                tracing::error!("Failed to disable auto-launch: {}", e);
            } else {
                tracing::info!("Auto-launch disabled");
                item.set_checked(false);
            }
        }
        false => {
            if let Err(e) = auto_launch.enable() {
                tracing::error!("Failed to enable auto-launch: {}", e);
            } else {
                tracing::info!("Auto-launch enabled");
                item.set_checked(true);
            }
        }
    }
}
