//! `iris-bench`, the command line tool.
//!
//! Only `check`, `clickbench`, `corpus`, `noise`, `overhead` and `resident` are implemented. See
//! `docs/ROADMAP.md` for the rest.

mod clickbench;
mod corpus;
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
    /// Get a corpus onto this machine and verify it against its manifest.
    ///
    /// A fetched corpus is downloaded and a generated one is produced by a generator the manifest
    /// names. Either way the digest is checked in the same pass that writes the file, and the
    /// asserted row and column counts are checked after. A corpus that is not what the manifest
    /// says never reaches a measurement, which is the only reason any number here is worth
    /// comparing to a later one.
    Corpus {
        /// Corpus name, as it appears in the manifest directory.
        name: String,
        /// Where the corpus manifests are, when they are not in `corpora/`.
        #[arg(long, value_name = "PATH")]
        root: Option<PathBuf>,
        /// Where the bytes go, when they are not going in `corpus/`.
        ///
        /// Worth pointing somewhere with room. The store is content addressed, so pointing several
        /// checkouts at one store is the supported way to have a corpus once rather than per clone.
        #[arg(long, value_name = "PATH")]
        store: Option<PathBuf>,
        /// Where the generator is, for a generated corpus whose program is not on `PATH`.
        ///
        /// The usual case, since TPC's `dbgen` is built in a directory of its own and is not
        /// something anybody installs.
        #[arg(long, value_name = "PATH")]
        generator: Option<PathBuf>,
        /// Where a generator is allowed to write, when that is not `corpus-scratch/`.
        ///
        /// Needs room for the whole corpus. A generator writes what it writes and only then can any
        /// of it be checked, so this is a second copy for as long as the generation takes.
        #[arg(long, value_name = "PATH")]
        scratch: Option<PathBuf>,
    },
    /// Run the 43 published `ClickBench` queries against one system, or compare runs.
    Clickbench {
        /// What to do.
        #[command(subcommand)]
        what: ClickbenchCommand,
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

/// The four parts of `clickbench`.
///
/// Running and comparing are separate because the systems are measured one at a time, often on
/// different days, and a comparison that could only happen inside a run would be a comparison that
/// never happened. Calibrating is separate from both because it reads every record of a run
/// together, and a single record cannot tell a misconfiguration apart from a slower machine.
/// Answering is separate from all three because it is what somebody reaches for after a comparison
/// failed, and it takes no timings at all.
#[derive(Debug, Subcommand)]
enum ClickbenchCommand {
    /// Measure one system.
    Run {
        /// Which system: `duckdb`, `datafusion` or `arrow-parquet`.
        #[arg(long)]
        driver: String,
        /// The Parquet file holding the hits table, already fetched and already verified.
        #[arg(long, value_name = "PATH")]
        file: PathBuf,
        /// Where to write the record.
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        /// The seed to replay, as sixteen lower case hex characters. A fresh one when absent.
        #[arg(long)]
        seed: Option<bench_run::Seed>,
        /// Which pass over the workload this is, when running it more than once under one seed.
        #[arg(long, default_value_t = 0)]
        pass: u32,
        /// Run them in the order the file lists them, which is what randomised ordering is for
        /// avoiding. For reproducing somebody else's protocol rather than for producing a number.
        #[arg(long)]
        in_order: bool,
        /// How many threads the system is allowed. Defaults to the whole machine, which is what the
        /// leaderboard entries are taken with.
        #[arg(long)]
        threads: Option<usize>,
        /// How many gibibytes the system is allowed.
        #[arg(long, default_value_t = 16)]
        memory: u64,
        /// Where the system may write, when that is not `run-scratch/`.
        #[arg(long, value_name = "PATH")]
        scratch: Option<PathBuf>,
        /// Drop the page cache before each query, which is what makes the cold column cold. Needs
        /// root on Linux, and says so rather than pretending when it does not have it.
        #[arg(long)]
        cold: bool,
        /// Measure even though the machine failed its gates, which produces a number nobody should
        /// put next to a leaderboard.
        #[arg(long)]
        anyway: bool,
    },
    /// Print what one system actually returns for named queries, rather than what it hashes to.
    Answer {
        /// Which system: `duckdb`, `datafusion` or `arrow-parquet`.
        #[arg(long)]
        driver: String,
        /// The Parquet file holding the hits table, already fetched and already verified.
        #[arg(long, value_name = "PATH")]
        file: PathBuf,
        /// Which queries, by the id the record calls them, such as `q23`. Repeat for more.
        #[arg(long, required = true, value_name = "ID")]
        query: Vec<String>,
        /// How many rows of each answer to print before saying how many were left.
        #[arg(long, default_value_t = 40)]
        rows: usize,
        /// How many threads the system is allowed. Defaults to the whole machine.
        #[arg(long)]
        threads: Option<usize>,
        /// How many gibibytes the system is allowed.
        #[arg(long, default_value_t = 16)]
        memory: u64,
        /// Where the system may write, when that is not `run-scratch/`.
        #[arg(long, value_name = "PATH")]
        scratch: Option<PathBuf>,
    },
    /// Read records back and say whether the systems agreed about the answers.
    Check {
        /// The records to compare, two or more.
        #[arg(required = true, num_args = 2..)]
        records: Vec<PathBuf>,
    },
    /// Read records back and say whether any system is out of line with the public leaderboard.
    Calibrate {
        /// The records from one run, two or more, all from the same machine.
        #[arg(required = true, num_args = 2..)]
        records: Vec<PathBuf>,
        /// Which column to compare. Hot isolates the engine from the machine's disk, which is why
        /// it is the default.
        #[arg(long, value_enum, default_value_t = Which::Hot)]
        column: Which,
    },
}

/// Which column of the protocol to calibrate against, as a command line argument.
///
/// A separate type from `bench_workload::Column` so that clap's spelling of the choices is this
/// crate's business rather than something a library crate has to know about.
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
enum Which {
    /// The first of the three runs.
    Cold,
    /// The best of the runs after the first.
    Hot,
}

impl From<Which> for bench_workload::Column {
    fn from(which: Which) -> Self {
        match which {
            Which::Cold => Self::Cold,
            Which::Hot => Self::Hot,
        }
    }
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

/// Hands one of the four `clickbench` subcommands its arguments.
///
/// Its own function rather than an arm of the match in `main`, because the run alone has eleven
/// arguments and burying four commands' worth of that inside another match makes the one place a
/// reader goes to find out what a flag does the hardest place in the crate to read.
fn clickbench(what: ClickbenchCommand) -> anyhow::Result<()> {
    // Absent means the whole machine, which is what the leaderboard entries are taken with, and it
    // is resolved here rather than in the module so that the record has a number in it either way.
    let whole = || std::thread::available_parallelism().map_or(1, Into::into);
    match what {
        ClickbenchCommand::Run {
            driver,
            file,
            out,
            seed,
            pass,
            in_order,
            threads,
            memory,
            scratch,
            cold,
            anyway,
        } => clickbench::run(
            &driver,
            &file,
            out,
            seed,
            pass,
            in_order,
            threads.unwrap_or_else(whole),
            memory * (1 << 30),
            scratch,
            cold,
            anyway,
        ),
        ClickbenchCommand::Answer {
            driver,
            file,
            query,
            rows,
            threads,
            memory,
            scratch,
        } => clickbench::answer(
            &driver,
            &file,
            &query,
            rows,
            threads.unwrap_or_else(whole),
            memory * (1 << 30),
            scratch,
        ),
        ClickbenchCommand::Check { records } => clickbench::check(&records),
        ClickbenchCommand::Calibrate { records, column } => {
            clickbench::calibrate(&records, column.into())
        }
    }
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
        Command::Clickbench { what } => clickbench(what),
        Command::Corpus {
            name,
            root,
            store,
            generator,
            scratch,
        } => corpus::run(&name, root, store, generator.as_deref(), scratch),
        other => anyhow::bail!("not implemented yet: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::Cli;

    #[test]
    fn the_command_line_is_well_formed() {
        // clap catches its own misconfiguration here rather than at the moment somebody runs the
        // tool on a machine that took an hour to get the corpus onto.
        Cli::command().debug_assert();
    }
}
