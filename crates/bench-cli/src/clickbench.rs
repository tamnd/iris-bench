//! `iris-bench clickbench`, which points one system at a real corpus and writes down what happened.
//!
//! Everything the run does is decided by crates that can be read on their own. `bench-workload`
//! holds the queries and the protocol, `bench-run` holds the schedule and the cache dropping, and
//! this module is the wiring: it turns a driver name into a driver, hands it the file, writes the
//! result out as JSON, and prints enough of it that somebody watching a terminal knows whether to
//! keep watching.
//!
//! # Why the file is taken as a path
//!
//! A driver never fetches anything and neither does this. `iris-bench corpus` obtains the corpus and
//! checks it against its manifest, and by the time a run starts the file is on the machine and has
//! already been verified. Doing the verification again here would mean reading fourteen gigabytes
//! before every run to re-answer a question that was already answered, so the record notes the path
//! and the byte count and leaves the digest to the command whose job it is.
//!
//! # What the summary does not do
//!
//! It prints a geometric mean over the queries that were answered and it prints how many those
//! were. It does not compare that number to the public leaderboard, because that comparison needs
//! the leaderboard's own numbers for the same machine class and a band to judge them against, and
//! that is its own piece of work rather than a line of arithmetic hidden in a print statement.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use bench_driver::{Driver, Format, Load, Session, Setup};
use bench_env::{Capture, Permit};
use bench_run::{DropCaches, Schedule, Scheduled, Seed};
use bench_workload::{Outcome, Warm, clickbench, compare};

/// One run of the workload against one system, and everything needed to read it later.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Record {
    /// What class of machine this was taken on.
    pub(crate) machine: String,
    /// The environment hash, which is what says two runs are comparable.
    pub(crate) environment: String,
    /// Where the corpus was and how large it was.
    pub(crate) corpus: Corpus,
    /// How long the system took to start, in nanoseconds.
    pub(crate) prepare_nanoseconds: f64,
    /// How long the system took to take the table, in nanoseconds.
    pub(crate) load_nanoseconds: f64,
    /// The schedule and the per query results.
    pub(crate) scheduled: Scheduled,
}

/// Which bytes were measured.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Corpus {
    /// Where the file was on the machine that ran this.
    pub(crate) path: String,
    /// How many bytes it held.
    pub(crate) bytes: u64,
}

/// Runs the workload and writes the record.
#[expect(
    clippy::too_many_arguments,
    reason = "every one of these is a run parameter that has to be visible in the command line \
              rather than defaulted somewhere a reader of the result cannot see"
)]
pub(crate) fn run(
    driver: &str,
    file: &Path,
    out: Option<PathBuf>,
    seed: Option<Seed>,
    pass: u32,
    in_order: bool,
    threads: usize,
    memory: u64,
    scratch: Option<PathBuf>,
    cold: bool,
    anyway: bool,
) -> anyhow::Result<()> {
    let capture = Capture::take();
    // ClickBench numbers are absolute durations, so this is the strict permit. Refusing by default
    // is the point: a number taken on a machine that failed its own gates is a number somebody will
    // put in a table anyway unless the tool stops them.
    if let Err(error) = capture.require(Permit::Durations) {
        if anyway {
            println!("running anyway on a machine that is only good for ratios: {error}");
        } else {
            return Err(error).context("run with --anyway to measure regardless");
        }
    }

    let bytes = std::fs::metadata(file)
        .with_context(|| format!("reading {}", file.display()))?
        .len();
    let dialect = clickbench::dialect(driver)
        .with_context(|| format!("no published ClickBench query set is chosen for {driver}"))?;
    let workload = clickbench::workload(dialect);
    let seed = seed.unwrap_or_else(Seed::fresh);

    let directory = scratch.unwrap_or_else(|| PathBuf::from("run-scratch").join(driver));
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("making {}", directory.display()))?;

    println!(
        "{driver} on {} bytes, {} queries",
        bytes,
        workload.queries.len()
    );
    println!(
        "seed {seed} pass {pass}{}",
        if in_order { ", in file order" } else { "" }
    );

    let mut system = system(driver)?;
    let mut session = Session::new(system.as_mut());
    let prepare_nanoseconds = session.prepare(&Setup {
        directory,
        threads,
        memory,
    })?;
    let load_nanoseconds = session.load(&Load {
        table: clickbench::TABLE.to_owned(),
        files: vec![file.to_owned()],
        format: Format::Parquet,
    })?;
    println!(
        "prepared in {:.1}s, loaded in {:.1}s",
        prepare_nanoseconds / 1e9,
        load_nanoseconds / 1e9
    );

    // Two page caches, and which one is used is a command line argument rather than a guess about
    // whether this process is root. A run that quietly measured warm and called it cold would be
    // worse than a run that refused.
    let mut dropping = DropCaches;
    let mut warm = Warm;
    let cache: &mut dyn bench_workload::PageCache = if cold { &mut dropping } else { &mut warm };

    let scheduled = if in_order {
        // Still a schedule, so the record has the same shape either way. The identity order is
        // recorded as the identity order rather than left out, because a missing field reads as a
        // run whose order nobody wrote down.
        Scheduled {
            schedule: Schedule::identity(workload.queries.len()),
            report: bench_workload::measure(&mut session, &workload, cache),
        }
    } else {
        bench_run::run(&mut session, &workload, cache, seed, pass)
    };

    let record = Record {
        machine: capture.class.to_string(),
        environment: capture.hash.to_string(),
        corpus: Corpus {
            path: file.display().to_string(),
            bytes,
        },
        prepare_nanoseconds,
        load_nanoseconds,
        scheduled,
    };
    summarise(&record);

    if let Some(path) = out {
        std::fs::write(&path, serde_json::to_string_pretty(&record)?)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("written to {}", path.display());
    }
    Ok(())
}

/// Reads several records back and says whether the systems agreed.
///
/// Separate from the run because the systems are run one at a time, often on different days, and a
/// comparison that could only happen inside a run would be a comparison that never happened.
pub(crate) fn check(paths: &[PathBuf]) -> anyhow::Result<()> {
    let mut records = Vec::with_capacity(paths.len());
    for path in paths {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let record: Record = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a clickbench record", path.display()))?;
        records.push(record);
    }

    let environments: Vec<&str> = records
        .iter()
        .map(|record| record.environment.as_str())
        .collect();
    if environments.windows(2).any(|pair| pair[0] != pair[1]) {
        // Not an error. Digests are supposed to agree across machines, and comparing two runs taken
        // on different ones is a real thing to want. Timings taken on different machines are not
        // comparable, and this is where somebody finds that out.
        println!("these runs are from different environments, so only the answers compare");
    }

    let reports: Vec<_> = records
        .into_iter()
        .map(|record| record.scheduled.report)
        .collect();
    let comparison = compare(&reports);

    println!("{} agreed", comparison.agreed.len());
    for one in &comparison.disagreed {
        println!("{} disagreed", one.query);
        for reading in &one.readings {
            println!(
                "    {:<16} {} rows {}",
                reading.driver,
                reading.rows,
                reading.digest.short()
            );
        }
    }
    for one in &comparison.unstable {
        println!("{} disagreed with itself on {}", one.driver, one.query);
    }
    if !comparison.alone.is_empty() {
        println!(
            "{} answered by one system only, so nothing checked them: {}",
            comparison.alone.len(),
            comparison.alone.join(" ")
        );
    }
    if !comparison.unanswered.is_empty() {
        println!(
            "{} answered by nothing: {}",
            comparison.unanswered.len(),
            comparison.unanswered.join(" ")
        );
    }

    anyhow::ensure!(comparison.clean(), "the systems did not agree");
    println!("every query that more than one system answered got the same answer");
    Ok(())
}

/// Turns a name into a system.
///
/// The three that have drivers. A name with no driver is refused here rather than defaulted to one
/// of them, for the same reason `clickbench::dialect` refuses an unknown name: picking a system on
/// somebody's behalf is how a result ends up describing something other than what it says it does.
fn system(name: &str) -> anyhow::Result<Box<dyn Driver>> {
    Ok(match name {
        "duckdb" => Box::new(driver_duckdb::DuckDb::new()),
        "datafusion" => Box::new(driver_datafusion::DataFusion::new()),
        "arrow-parquet" => Box::new(driver_arrow_parquet::ArrowParquet::default()),
        other => anyhow::bail!("no driver called {other}"),
    })
}

/// Prints the per query table and the two geometric means.
fn summarise(record: &Record) {
    let report = &record.scheduled.report;
    println!();
    println!("{:<6} {:>12} {:>12}  answer", "query", "cold", "hot");
    for query in &report.queries {
        match &query.outcome {
            Outcome::Answered { .. } => {
                let (cold, hot) = (query.cold().unwrap_or(0.0), query.hot().unwrap_or(0.0));
                let steady = if query.steady() { "" } else { "  unsteady" };
                println!(
                    "{:<6} {:>11.3}s {:>11.3}s  {}{steady}",
                    query.id,
                    cold / 1e9,
                    hot / 1e9,
                    query.digest().map_or_else(String::new, |one| one.short()),
                );
            }
            Outcome::Unsupported { why } => {
                println!("{:<6} {:>26}  {why}", query.id, "unsupported");
            }
            Outcome::Failed { why } => println!("{:<6} {:>26}  {why}", query.id, "failed"),
        }
    }

    println!();
    println!("{} of {} answered", report.answered(), report.queries.len());
    // Named for what it is. A geometric mean over the queries a system could answer is not the same
    // measure as one over all forty three, and calling both of them the geomean is how two of them
    // end up in one table.
    if let (Some(cold), Some(hot)) = (
        geomean(report.queries.iter().filter_map(cold)),
        geomean(report.queries.iter().filter_map(hot)),
    ) {
        println!(
            "geometric mean over those {}: {:.4}s cold, {:.4}s hot",
            report.answered(),
            cold / 1e9,
            hot / 1e9
        );
    }
    if !record.scheduled.report.queries.is_empty() && !was_cold(record) {
        println!("the page cache was not dropped, so the cold column is not a cold number");
    }
}

/// The cold time of one query, when it has one.
fn cold(query: &bench_workload::Measured) -> Option<f64> {
    query.cold()
}

/// The hot time of one query, when it has one.
fn hot(query: &bench_workload::Measured) -> Option<f64> {
    query.hot()
}

/// Whether every query in the run started with the page cache actually empty.
fn was_cold(record: &Record) -> bool {
    record
        .scheduled
        .report
        .queries
        .iter()
        .all(|query| query.cache.is_cold())
}

/// The geometric mean, or nothing when there is nothing to average.
///
/// Taken in log space because forty three durations in nanoseconds multiplied together overflow an
/// `f64` long before the root is taken.
fn geomean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0u32;
    for value in values {
        if value <= 0.0 {
            return None;
        }
        sum += value.ln();
        count += 1;
    }
    (count > 0).then(|| (sum / f64::from(count)).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_geometric_mean_is_the_geometric_mean() {
        let mean = geomean([1.0, 4.0, 16.0].into_iter()).unwrap();
        assert!((mean - 4.0).abs() < 1e-9, "{mean}");
    }

    #[test]
    fn nothing_to_average_is_not_a_zero() {
        assert!(geomean(std::iter::empty()).is_none());
        // A zero duration would send the log to negative infinity and the mean to zero, which reads
        // as an infinitely fast system rather than as the broken clock it is.
        assert!(geomean([1.0, 0.0].into_iter()).is_none());
    }

    #[test]
    fn a_name_with_no_driver_is_refused_rather_than_defaulted() {
        assert!(system("duckdb").is_ok());
        assert!(system("clickhouse").is_err());
    }
}
