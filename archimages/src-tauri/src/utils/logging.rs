//! tracing 日志：文件按日滚动 + 前端 log-event 推送。
//!
//! 红线：任何分支都不得输出 API Key 等凭据。所有日志在产出前先过
//! `redact_secrets`；密钥注册见 `register_secrets`。

use std::fmt;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

use crate::error::{AppError, Result};

/// 进程内已注册密钥；置空串直接跳过避免误伤。
static SECRETS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn secrets() -> &'static Mutex<Vec<String>> {
    SECRETS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 注册需要脱敏的密钥（Phase 12 保存 Google Key 时调用）。
pub fn register_secret(secret: &str) {
    if secret.is_empty() {
        return;
    }
    if let Ok(mut guard) = secrets().lock() {
        if !guard.iter().any(|s| s == secret) {
            guard.push(secret.to_string());
        }
    }
}

pub fn redact_secrets(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(secret, "***");
        }
    }
    out
}

fn redact_registered(text: &str) -> String {
    let owned: Vec<String> = secrets()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_else(|e| e.into_inner().clone());
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    redact_secrets(text, &refs)
}

/// 前端 LogPanel 消费的事件载荷。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEventDto {
    pub level: String,
    pub target: String,
    pub message: String,
}

/// 把 WARN/INFO 等事件实时推给前端的 tracing Layer。
/// webview 未就绪时 emit 失败被忽略，文件层仍在记录，不丢日志。
struct TauriEventLayer {
    app: AppHandle,
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let chunk = format!("{value:?}");
        if field.name() == "message" {
            self.message = chunk;
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            self.message.push_str(field.name());
            self.message.push('=');
            self.message.push_str(&chunk);
        }
    }
}

impl<S> Layer<S> for TauriEventLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let payload = LogEventDto {
            level: event.metadata().level().to_string(),
            target: event.metadata().target().to_string(),
            message: redact_registered(&visitor.message),
        };
        let _ = self.app.emit("log-event", payload);
    }
}

pub struct LogGuard {
    _file_guard: tracing_appender::non_blocking::WorkerGuard,
}

/// 初始化全局日志。失败即启动失败：数据安全工具不允许无审计日志运行。
pub fn init(log_dir: &Path, app_handle: AppHandle) -> Result<LogGuard> {
    std::fs::create_dir_all(log_dir)?;
    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("archimages")
        .filename_suffix("log")
        .build(log_dir)
        .map_err(|e| AppError::Config(format!("init log file appender: {e}")))?;
    let (non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);

    // 文件层在写出前统一脱敏（自定义 MakeWriter 包一层）。
    let redacting_writer = RedactingWriter {
        inner: non_blocking,
    };

    let filter = EnvFilter::try_from_env("ARCHIMAGES_LOG").unwrap_or_else(|_| {
        EnvFilter::new(if cfg!(debug_assertions) {
            "debug"
        } else {
            "info"
        })
    });

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(redacting_writer)
        .with_ansi(false);
    let stdout_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stdout);
    let event_layer = TauriEventLayer { app: app_handle };

    Registry::default()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .with(event_layer)
        .try_init()
        .map_err(|e| AppError::Config(format!("init tracing subscriber: {e}")))?;

    Ok(LogGuard {
        _file_guard: file_guard,
    })
}

struct RedactingWriter {
    inner: tracing_appender::non_blocking::NonBlocking,
}

struct RedactingIoWriter<'a> {
    inner: Box<dyn std::io::Write + 'a>,
}

impl std::io::Write for RedactingIoWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let redacted = redact_registered(&text);
        self.inner.write(redacted.as_bytes())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RedactingWriter {
    type Writer = RedactingIoWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingIoWriter {
            inner: Box::new(tracing_subscriber::fmt::MakeWriter::make_writer(
                &self.inner,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_registered_secret_in_text() {
        let out = redact_secrets("key=AIzaSySECRET123 ok", &["AIzaSySECRET123"]);
        assert_eq!(out, "key=*** ok");
    }

    #[test]
    fn empty_secret_is_ignored() {
        let out = redact_secrets("nothing to hide", &[""]);
        assert_eq!(out, "nothing to hide");
    }

    #[test]
    fn register_secret_then_redact_registered() {
        register_secret("unit-test-secret-xyz");
        let out = redact_registered("token unit-test-secret-xyz leaked");
        assert_eq!(out, "token *** leaked");
    }
}
