use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::process::ChildStderr;
use std::sync::OnceLock;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::runtime_paths;

static FILE_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
enum LogProfile {
    Dev,
    User,
    All,
}

pub fn init_logging() -> Result<()> {
    let profile = resolve_profile();
    let filter_spec = resolve_filter_spec(profile);
    let log_dir = runtime_paths::resolve_log_dir();
    std::fs::create_dir_all(&log_dir)
        .with_context(|| format!("failed to create log dir {}", log_dir.display()))?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "vibefi.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    FILE_GUARD
        .set(guard)
        .map_err(|_| anyhow::anyhow!("logging guard was already initialized"))?;

    // If another logger has already been installed, keep going; tracing still works.
    let _ = tracing_log::LogTracer::init();

    let env_filter = EnvFilter::try_new(filter_spec.clone())
        .with_context(|| format!("invalid log filter: {filter_spec}"))?;

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(true)
                .with_file(true)
                .with_line_number(true),
        )
        .with(
            fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false)
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .with_thread_ids(true)
                .with_thread_names(true),
        );
    tracing::subscriber::set_global_default(subscriber)
        .context("failed to initialize tracing subscriber")?;

    tracing::info!(
        profile = ?profile,
        filter = %filter_spec,
        log_dir = %log_dir.display(),
        "logging initialized"
    );
    Ok(())
}

pub fn forward_child_stderr(helper: &'static str, stderr: ChildStderr) {
    let thread_name = format!("{helper}-stderr-log");
    let _ = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        let msg = line.trim();
                        if msg.is_empty() {
                            continue;
                        }
                        let lower = msg.to_ascii_lowercase();
                        if lower.contains("fatal") || lower.contains("error") {
                            tracing::warn!(target: "vibefi::helper", helper = helper, "{msg}");
                        } else {
                            tracing::debug!(target: "vibefi::helper", helper = helper, "{msg}");
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            target: "vibefi::helper",
                            helper = helper,
                            error = %err,
                            "failed reading helper stderr"
                        );
                        break;
                    }
                }
            }
        });
}

fn resolve_profile() -> LogProfile {
    if let Ok(raw) = std::env::var("VIBEFI_LOG_PROFILE") {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dev" => return LogProfile::Dev,
            "user" => return LogProfile::User,
            "all" => return LogProfile::All,
            _ => {}
        }
    }

    if cfg!(debug_assertions) || std::env::var_os("CARGO").is_some() {
        LogProfile::Dev
    } else {
        LogProfile::User
    }
}

fn resolve_filter_spec(profile: LogProfile) -> String {
    if let Ok(raw) = std::env::var("RUST_LOG") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Ok(raw) = std::env::var("VIBEFI_LOG") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    match profile {
        LogProfile::Dev => "off,vibefi=trace,vibefi::helper=debug".to_string(),
        LogProfile::User => "info".to_string(),
        LogProfile::All => "trace".to_string(),
    }
}
