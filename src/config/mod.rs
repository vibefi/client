/// Layered configuration resolution for the VibeFi client.
///
/// Resolution order (later wins):
/// 1. Deployment JSON (`AppConfig`) — network/contract/IPFS defaults
/// 2. Environment variables (`VIBEFI_*`) — per-session overrides
/// 3. Compile-time flags (`cfg!(debug_assertions)`) — devtools default
///
/// User settings (`settings.rs`) are **not** folded into `ResolvedConfig`
/// because they can change at runtime via the settings panel. Consumers
/// that need user-overridable values (e.g. IPFS backend) merge them at
/// call sites.
mod app_config;
mod builder;
pub mod cli;
mod env;
mod resolved;
mod validation;

pub use app_config::{AppConfig, IpfsFetchBackend};
pub use builder::ConfigBuilder;
pub use cli::CliArgs;
pub use resolved::ResolvedConfig;

use anyhow::{Context, Result, anyhow};
use std::path::Path;

/// Load and validate an `AppConfig` from a JSON file.
pub fn load_config(path: &Path) -> Result<AppConfig> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read config file {}", path.display()))?;
    let cfg: AppConfig = serde_json::from_str(&raw).map_err(|err| {
        let kind = match err.classify() {
            serde_json::error::Category::Io => "I/O",
            serde_json::error::Category::Syntax => "syntax",
            serde_json::error::Category::Data => "data",
            serde_json::error::Category::Eof => "unexpected EOF",
        };

        anyhow!(
            "parse config file {} failed ({} error) at line {}, column {}: {}",
            path.display(),
            kind,
            err.line(),
            err.column(),
            err
        )
    })?;
    validation::validate_app_config(&cfg)?;
    Ok(cfg)
}
