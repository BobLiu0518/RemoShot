#[cfg(target_os = "macos")]
pub fn check_and_request_screen_recording_permission() -> bool {
    use screenshots::Screen;
    use std::process::Command;

    let has_permission = match Screen::all() {
        Ok(screens) => {
            if screens.is_empty() {
                false
            } else {
                screens[0].capture().is_ok()
            }
        }
        Err(_) => false,
    };

    if !has_permission {
        tracing::warn!("no screen recording permission detected, opening system preferences");

        let _ = Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .spawn();

        return false;
    }

    true
}

#[cfg(not(target_os = "macos"))]
pub fn check_and_request_screen_recording_permission() -> bool {
    true
}
