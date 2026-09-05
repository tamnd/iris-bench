//! `iris-bench`, the command line tool.
//!
//! Only `check`, `noise`, `overhead` and `resident` are implemented. See `docs/ROADMAP.md` for the
//! rest.

mod noise;
mod overhead;
mod resident;

use std::path::PathBuf;

use anyhow::Context as _;
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
    /// Measure how far this machine's answer moves between runs of one fixed workload.
    ///
    /// The number this prints is the floor under every effect measured here afterwards, so it is
    /// the first thing to run on a machine nobody has measured before and the first thing to run
    /// again when a result looks too good.
    Noise {
        /// How many times to start the process again.
        #[arg(long, default_value_t = 20)]
        rounds: u32,
        /// How many samples to take inside each round.
        #[arg(long, default_value_t = 50)]
        samples: u32,
        /// How many passes to run before a round starts recording.
        #[arg(long, default_value_t = 5)]
        warmup: u32,
        /// The spread above which a class is ratios only, as a fraction.
        #[arg(long, default_value_t = 0.02)]
        limit: f64,
        /// Measure even though a gate failed, which answers what a busy machine looks like.
        #[arg(long)]
        anyway: bool,
        /// Run one round and print what it measured, which is how a round is started.
        #[arg(long, hide = true)]
        one_round: bool,
    },
    /// Compare a scan of a resident local file against a scan of the same bytes already in memory.
    ///
    /// The M4 gate in iris. Unlike `noise`, being over the bar exits non zero, because this is a
    /// claim the project makes about its own design rather than a property of a machine.
    Resident {
        /// How large a file to scan, in mebibytes.
        #[arg(long, default_value_t = 256)]
        size: u64,
        /// How much address space the window reserves, in mebibytes.
        #[arg(long, default_value_t = 4)]
        span: u64,
        /// How large each range a scan asks for is, in kibibytes.
        #[arg(long, default_value_t = 256)]
        chunk: u64,
        /// How many pairs of measurements to take.
        #[arg(long, default_value_t = 60)]
        pairs: u32,
        /// How many scans of each side to run before recording anything.
        #[arg(long, default_value_t = 3)]
        warmup: u32,
        /// How far from the whole buffer path the windowed path may be, as a fraction.
        #[arg(long, default_value_t = 0.03)]
        bar: f64,
        /// Measure even though a gate failed, which produces a working note rather than a result.
        #[arg(long)]
        anyway: bool,
        /// Compare the buffer against a second buffer, which measures this command's own bias.
        ///
        /// Nothing about iris is under test in a control run. It answers what the same comparison
        /// reports when both sides are the same thing, which is the number every other ratio from
        /// this machine has to be read next to.
        #[arg(long)]
        control: bool,
    },
    /// Measure what the timing loop adds to a sample, and the workload length that makes it small.
    ///
    /// The B0 gate. There is no single overhead percentage, because the harness adds a roughly
    /// fixed number of nanoseconds and the share that takes depends on how long the workload runs.
    /// What this reports is the fixed cost and the duration at which it falls under the bar.
    Overhead {
        /// How many pairs of measurements to take at each duration.
        #[arg(long, default_value_t = 40)]
        pairs: u32,
        /// How many iterations of the workload each side of a pair runs.
        #[arg(long, default_value_t = 64)]
        batch: u32,
        /// How many pairs to run at each duration before recording anything.
        #[arg(long, default_value_t = 3)]
        warmup: u32,
        /// The share of a workload the harness may take, as a fraction.
        #[arg(long, default_value_t = 0.01)]
        bar: f64,
        /// The shortest workload this fleet intends to time, in microseconds.
        ///
        /// The gate is whether the harness is under the bar at this duration. It is an input
        /// because it is a decision about what gets benchmarked rather than a fact about the
        /// machine, and burying a decision like that in a constant makes it look like a fact.
        #[arg(long, default_value_t = 10)]
        floor: u32,
        /// Measure even though a gate failed, which produces a working note rather than a result.
        #[arg(long)]
        anyway: bool,
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
        Command::Noise {
            rounds,
            samples,
            warmup,
            limit,
            anyway,
            one_round,
        } => {
            if one_round {
                noise::one_round(samples, warmup)
            } else {
                noise::probe(rounds, samples, warmup, limit, anyway)
            }
        }
        Command::Resident {
            size,
            span,
            chunk,
            pairs,
            warmup,
            bar,
            anyway,
            control,
        } => {
            let mib = 1024 * 1024;
            resident::gate(
                size * mib,
                usize::try_from(span * mib).context("a window span that does not fit in memory")?,
                usize::try_from(chunk * 1024).context("a range that does not fit in memory")?,
                pairs,
                warmup,
                bar,
                anyway,
                control,
            )
        }
        Command::Overhead {
            pairs,
            batch,
            warmup,
            bar,
            floor,
            anyway,
        } => overhead::gate(
            pairs,
            batch,
            warmup,
            bar,
            f64::from(floor) * 1_000.0,
            anyway,
        ),
        other => anyhow::bail!("not implemented yet: {other:?}"),
    }
}
