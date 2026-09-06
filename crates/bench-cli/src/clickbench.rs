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
//! were. It does not compare that number to the public leaderboard. That comparison needs more than
//! one driver, because a single driver on its own cannot tell a misconfiguration apart from a
//! slower machine, so it lives in `calibrate` where all the records of a run are read together and
//! not in a line of arithmetic hidden in the summary of one of them.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use bench_driver::{Driver, Format, Load, Session, Setup};
use bench_env::{Capture, Permit};
use bench_run::{DropCaches, Schedule, Scheduled, Seed};
use bench_workload::leaderboard::{self, Calibration, Column, Standing};
use bench_workload::{Outcome, Warm, clickbench, compare, geomean};

/// One run of the workload against one system, and everything needed to read it later.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Record {
    /// What class of machine this was taken on.
    pub(crate) machine: String,
    /// The environment hash, which is what says two runs are comparable.
    pub(crate) environment: String,
    /// Where the corpus was and how large it was.
    pub(crate) corpus: Corpus,
    /// The setup script this system's own entry publishes, where there is one and this driver was
    /// given it.
    ///
    /// Recorded next to the query source and for the same reason. Two runs of the same queries can
    /// still have taken the table in differently, and a record that named only the queries would
    /// leave a reader unable to tell which of the two conversions produced the numbers under it.
    /// `None` means the files were read as they lie, which is the honest answer for a driver that
    /// builds no table.
    #[serde(default)]
    pub(crate) setup: Option<bench_workload::Source>,
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

    // The setup this system's own entry publishes, where it has one. The two entries in scope
    // convert different columns and pay for it at different times, so which one was applied is part
    // of what the record has to say rather than something a reader works out from the driver name.
    let projection = clickbench::projection(driver);
    let setup = projection
        .and(dialect.setup_source())
        .inspect(|source| println!("setup from {}", source.url));

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
        projection: projection.map(str::to_owned),
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
        setup,
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

/// Runs named queries once each and prints what came back.
///
/// This is the command for after `check` failed. A digest says two systems differ and says nothing
/// at all about what they differ on, and a benchmark that hashes an answer and throws it away
/// leaves the reader guessing at exactly the moment guessing is most expensive. So this takes the
/// same table in the same way a run does, runs the queries the comparison complained about, and
/// prints the canonical rendering both digests were taken over.
///
/// It takes no timings, so it asks for no permit and it is fine on a busy machine. Nothing it
/// prints can end up in a table of numbers.
pub(crate) fn answer(
    driver: &str,
    file: &Path,
    wanted: &[String],
    rows: usize,
    threads: usize,
    memory: u64,
    scratch: Option<PathBuf>,
) -> anyhow::Result<()> {
    let dialect = clickbench::dialect(driver)
        .with_context(|| format!("no published ClickBench query set is chosen for {driver}"))?;
    let workload = clickbench::workload(dialect);
    for id in wanted {
        anyhow::ensure!(
            workload.queries.iter().any(|query| &query.id == id),
            "{driver} has no query called {id}"
        );
    }

    let directory = scratch.unwrap_or_else(|| PathBuf::from("run-scratch").join(driver));
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("making {}", directory.display()))?;

    let projection = clickbench::projection(driver);
    let mut system = system(driver)?;
    let mut session = Session::new(system.as_mut());
    session.prepare(&Setup {
        directory,
        threads,
        memory,
    })?;
    session.load(&Load {
        table: clickbench::TABLE.to_owned(),
        files: vec![file.to_owned()],
        format: Format::Parquet,
        projection: projection.map(str::to_owned),
    })?;

    // In the order they were asked for rather than in workload order, because the caller is holding
    // two of these side by side and wants the same query in the same place in both.
    for id in wanted {
        let query = workload
            .queries
            .iter()
            .find(|query| &query.id == id)
            .expect("the ids were checked above");
        println!("\n{id} {}", query.sql);
        match session.query(query) {
            Ok((answer, _)) => {
                println!(
                    "{} rows, {} columns, digest {}",
                    answer.rows,
                    answer.columns,
                    bench_workload::Digest::of(&answer)
                );
                let lines: Vec<&str> = answer.body.lines().collect();
                for line in lines.iter().take(rows) {
                    println!("  {line}");
                }
                if let Some(hidden) = lines.len().checked_sub(rows).filter(|left| *left > 0) {
                    println!("  and {hidden} more rows, raise --rows to see them");
                }
            }
            Err(error) => println!("did not answer: {error}"),
        }
    }
    Ok(())
}

/// Reads several records back and says whether the systems agreed.
///
/// Separate from the run because the systems are run one at a time, often on different days, and a
/// comparison that could only happen inside a run would be a comparison that never happened.
pub(crate) fn check(paths: &[PathBuf]) -> anyhow::Result<()> {
    let records = read(paths)?;

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

/// Reads several records back and says whether any system is out of line with the leaderboard.
///
/// Every record from one run goes in at once, because the machine here is not the machine upstream
/// used and the only way to tell a misconfigured driver from a slower machine is to see whether the
/// other drivers moved with it. `bench_workload::leaderboard` holds the published numbers and does
/// the arithmetic; this prints it.
pub(crate) fn calibrate(paths: &[PathBuf], column: Column) -> anyhow::Result<()> {
    let records = read(paths)?;
    let environments: Vec<&str> = records
        .iter()
        .map(|record| record.environment.as_str())
        .collect();
    anyhow::ensure!(
        environments.windows(2).all(|pair| pair[0] == pair[1]),
        "these records are from different environments, and a machine factor estimated across two \
         machines is not a machine factor"
    );
    if column == Column::Cold && !records.iter().all(was_cold) {
        println!("the page cache was not dropped everywhere, so the cold column is not cold here");
    }

    let reports: Vec<&bench_workload::Report> = records
        .iter()
        .map(|record| &record.scheduled.report)
        .collect();
    let calibration = Calibration::new(&reports, column)
        .context("no driver in these records has a published result to compare against")?;

    println!();
    println!(
        "{:<16} {:>10} {:>10} {:>8} {:>8}  reference",
        "driver",
        format!("ours {column}"),
        "theirs",
        "ratio",
        "drift"
    );
    for standing in &calibration.standings {
        let published = standing.reference.published();
        println!(
            "{:<16} {:>9.4}s {:>9.4}s {:>7.2}x {:>7.0}%  {} on {} {}",
            standing.driver,
            standing.ours,
            standing.theirs,
            standing.ratio,
            calibration.drift(standing) * 100.0,
            standing.system,
            standing.machine,
            published.date,
        );
    }
    for driver in &calibration.unmatched {
        println!("{driver:<16} no published result, so nothing to compare it against");
    }
    println!();
    println!(
        "this machine is {:.2}x the published numbers, and a driver is in band within {:.0}% of \
         that",
        calibration.machine,
        leaderboard::BAND * 100.0
    );

    for standing in calibration.outside() {
        println!();
        println!(
            "{} is {:.0}% off the shared factor, which is a misconfiguration until somebody shows \
             it is not",
            standing.driver,
            calibration.drift(standing) * 100.0
        );
        worst(standing);
    }

    anyhow::ensure!(calibration.clean(), "a driver is outside the band");
    println!("every driver with a published result is inside the band");
    Ok(())
}

/// Prints the queries furthest out of line with the driver's own ratio.
///
/// A failed gate is only actionable if it names something. A driver that is uniformly slow and a
/// driver that is fine except for four queries are the same number at the top of the table and
/// completely different problems underneath it.
fn worst(standing: &Standing) {
    println!(
        "{:<6} {:>10} {:>10} {:>8}",
        "query", "ours", "theirs", "relative"
    );
    for point in standing.points.iter().take(5) {
        println!(
            "{:<6} {:>9.4}s {:>9.4}s {:>7.1}x",
            point.id, point.ours, point.theirs, point.relative
        );
    }
}

/// Reads records off disk, in the order they were named.
fn read(paths: &[PathBuf]) -> anyhow::Result<Vec<Record>> {
    let mut records = Vec::with_capacity(paths.len());
    for path in paths {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let record: Record = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a clickbench record", path.display()))?;
        records.push(record);
    }
    Ok(records)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_with_no_driver_is_refused_rather_than_defaulted() {
        assert!(system("duckdb").is_ok());
        assert!(system("clickhouse").is_err());
    }

    #[test]
    fn answer_checks_the_query_ids_before_it_touches_the_corpus() {
        // Fourteen gigabytes take a while to open and the mistake here is a typed query id, so the
        // ids are checked against the workload first. The file below does not exist, and the error
        // that comes back is about the id rather than about the file, which is what says the order
        // is the one intended rather than the one that happened.
        let error = answer(
            "duckdb",
            Path::new("/nonexistent/hits.parquet"),
            &["q43".to_owned()],
            10,
            1,
            1 << 30,
            None,
        )
        .expect_err("q43 is one past the end of the workload");
        assert!(error.to_string().contains("no query called q43"), "{error}");
    }
}
