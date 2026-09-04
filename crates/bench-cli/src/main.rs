//! `iris-bench`, the command line tool.
//!
//! Nothing is implemented yet. See `docs/ROADMAP.md`.

use clap::{Parser, Subcommand};

/// Command line interface for the iris-bench harness.
#[derive(Debug, Parser)]
#[command(name = "iris-bench", version, about, long_about = None)]
struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    command: Command,
}

/// The subcommands `iris-bench` understands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Check whether this machine is fit to produce a publishable number.
    Check,
    /// Fetch or generate a corpus and verify it against its manifest.
    Corpus {
        /// Corpus name, as it appears in the manifest directory.
        name: String,
    },
    /// Run a workload and append the results to the store.
    Run {
        /// Path to a run manifest.
        manifest: String,
    },
    /// Re-run a published reproduction target and record a verdict.
    Reproduce {
        /// Target identifier, for example `f3` or `alp`.
        target: String,
    },
    /// Render the store.
    Report,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    anyhow::bail!("not implemented yet: {:?}", cli.command)
}
