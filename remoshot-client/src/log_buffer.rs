use std::fmt;
use std::sync::{Arc, Mutex};
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

type UpdateCallback = Box<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<LogBufferInner>>,
}

struct LogBufferInner {
    lines: Vec<String>,
    capacity: usize,
    update_callback: Option<Arc<UpdateCallback>>,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LogBufferInner {
                lines: Vec::with_capacity(capacity),
                capacity,
                update_callback: None,
            })),
        }
    }

    pub fn set_update_callback<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut inner = self.inner.lock().unwrap();
        inner.update_callback = Some(Arc::new(Box::new(callback)));
    }

    pub fn push(&self, line: String) {
        let callback = {
            let mut inner = self.inner.lock().unwrap();
            if inner.lines.len() >= inner.capacity {
                inner.lines.remove(0);
            }
            inner.lines.push(line);
            inner.update_callback.clone()
        };

        if let Some(callback) = callback {
            callback();
        }
    }

    pub fn snapshot(&self) -> String {
        let inner = self.inner.lock().unwrap();
        inner.lines.join("\n")
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.lines.clear();
    }
}

pub struct LogBufferLayer {
    buffer: LogBuffer,
}

impl LogBufferLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

struct MessageVisitor {
    message: String,
    fields: Vec<String>,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else {
            self.fields.push(format!("{}={:?}", field.name(), value));
        }
    }
}

impl<S: Subscriber> Layer<S> for LogBufferLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let level = meta.level();
        let target = meta.target();

        let mut visitor = MessageVisitor {
            message: String::new(),
            fields: Vec::new(),
        };
        event.record(&mut visitor);

        let now = chrono::Local::now().format("%H:%M:%S");
        let line = if visitor.fields.is_empty() {
            format!("[{now} {level} {target}] {}", visitor.message)
        } else {
            format!(
                "[{now} {level} {target}] {} {}",
                visitor.message,
                visitor.fields.join(" ")
            )
        };

        self.buffer.push(line);
    }
}
