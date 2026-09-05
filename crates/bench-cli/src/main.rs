//! `iris-bench`, the command line tool.
//!
//! Only `check` is implemented. See `docs/ROADMAP.md` for the rest.

use std::path::PathBuf;

use bench_env::{Capture, Permit};
use clap::{Parser, Subcommand, ValueEnum};

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
    Check {
        /// What the machine would have to be good enough for.
        ///
        /// Exits non zero when it is not, which is what makes this usable as the first step of a
        /// run rather than as something a person reads and then ignores.
        #[arg(long, value_enum, default_value_t = Requirement::Durations)]
        require: Requirement,
        /// Where to write the environment capture.
        ///
        /// The capture is written whether or not the machine passes, because a refused run is
        /// exactly the case where somebody wants to see what was read.
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
    },
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

/// What a caller needs the machine to be good enough for.
///
/// The same three levels as [`Permit`], separately declared because a command line argument is a
/// stable interface and the internal vocabulary is free to move.
#[derive(Clone, Copy, PartialEq, Eq, Debug, ValueEnum)]
enum Requirement {
    /// Absolute durations, which is the only thing comparable across machines.
    Durations,
    /// Ratios taken inside one run, where the machine is its own control.
    Ratios,
}

impl From<Requirement> for Permit {
    fn from(requirement: Requirement) -> Self {
        match requirement {
            Requirement::Durations => Self::Durations,
            Requirement::Ratios => Self::Ratios,
        }
    }
}

/// Reads the machine, says what it is good for, and refuses by name when that is not enough.
fn check(require: Requirement, out: Option<PathBuf>) -> anyhow::Result<()> {
    let capture = Capture::take();

    println!("{}", capture.class);
    println!("environment {}", capture.hash);
    for gate in &capture.gates {
        let (mark, detail) = match &gate.outcome {
            bench_env::Outcome::Pass { observed } => ("pass", observed.clone()),
            bench_env::Outcome::Fail { observed, wanted } => {
                ("FAIL", format!("{observed}, wanted {wanted}"))
            }
        };
        println!("  {mark}  {:<20} {detail}", gate.name);
    }
    println!("this machine is good for {}", capture.permits());

    if let Some(path) = out {
        std::fs::write(&path, capture.to_json())?;
        println!("capture written to {}", path.display());
    }

    // The refusal is returned rather than printed, so that the exit status and the message come from
    // the same place a library caller would get them from.
    Ok(capture.require(require.into())?)
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Check { require, out } => check(require, out),
        other => anyhow::bail!("not implemented yet: {other:?}"),
    }
}
