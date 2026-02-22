use clap::Parser;
use std::path::PathBuf;

/// VibeFi — decentralised application browser.
#[derive(Debug, Parser)]
#[command(name = "vibefi", about)]
pub struct CliArgs {
    /// Path to a local dapp project directory to bundle and serve.
    #[arg(long)]
    pub bundle: Option<PathBuf>,

    /// Path to a local studio bundle directory used for Studio tab dev loading.
    #[arg(long = "studio-bundle")]
    pub studio_bundle: Option<PathBuf>,

    /// Path to the network config JSON file (e.g. config/sepolia.json).
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Skip the `bun build` step when using --bundle.
    #[arg(long)]
    pub no_build: bool,

    /// Enable automation mode (NDJSON commands on stdin, results on stdout).
    #[arg(long)]
    pub automation: bool,
}
