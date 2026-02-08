use screenshots::Screen;
use std::io::Cursor;

use remoshot_common::ScreenshotData;

pub fn capture_all_screens() -> Vec<ScreenshotData> {
    let screens = match Screen::all() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("failed to enumerate screens: {}", e);
            return Vec::new();
        }
    };

    let mut results = Vec::new();

    for (i, screen) in screens.iter().enumerate() {
        match screen.capture() {
            Ok(img) => {
                let mut jpeg_buf = Cursor::new(Vec::new());
                let encoder = screenshots::image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut jpeg_buf,
                    80,
                );
                if let Err(e) = img.write_with_encoder(encoder) {
                    tracing::error!("failed to encode screenshot {}: {}", i, e);
                    continue;
                }

                results.push(ScreenshotData {
                    monitor: i as u32,
                    data: jpeg_buf.into_inner(),
                });
            }
            Err(e) => {
                tracing::error!("failed to capture screen {}: {}", i, e);
            }
        }
    }

    results
}
