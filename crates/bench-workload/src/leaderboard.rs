//! The published numbers, so ours can be checked against them.
//!
//! `ClickBench` publishes every result its leaderboard has ever shown, as one file per system, per
//! machine class, per date. This module carries two of those files verbatim and puts our numbers
//! next to them. That comparison is the calibration gate for everything else in this repository. If
//! a system we configured is much slower here than the same system is upstream, we configured it
//! wrong, and every comparison published afterwards is measuring our configuration mistake rather
//! than anything about the systems.
//!
//! # The hardware is not the same and that is the whole difficulty
//!
//! Upstream's numbers are taken on rented machines of a named class and ours are not, so a ratio of
//! one is not what a correct configuration looks like and neither is any other particular number.
//! What is true is that a faster machine moves every query on every system by roughly one factor.
//! So each driver gets the ratio of our geometric mean to the published one, the machine factor is
//! the geometric mean of those ratios across the drivers, and a driver is in band when its own
//! ratio is within [`BAND`] of the shared factor. The factor absorbs the hardware and what is left
//! is the part that belongs to one driver, which is where a misconfiguration lives.
//!
//! # What this cannot catch, said plainly
//!
//! A mistake that slows every driver here by the same amount is arithmetically indistinguishable
//! from a slower machine, and this check passes on it. A corpus on a slower disk than upstream's
//! and a machine with less memory than the working set are both that kind of mistake. Neither is
//! caught here and neither is meant to be, because the environment capture in `bench-env` and
//! [`Cold`] are where those show up instead. What this does catch is one driver out of line with
//! the others, which is what a wrong thread count, a wrong memory limit or a corpus laid out one
//! way for one driver and another way for the next looks like from the outside.
//!
//! It also means the check gets weaker the fewer drivers there are. With one driver it is empty,
//! because the shared factor is that driver's own ratio. Two engines is what this repository has
//! today and two is enough to see one of them drift, which is the failure that actually happens.
//!
//! # It says nothing about answers
//!
//! A system returning the wrong rows quickly lands comfortably inside the band. [`crate::agree`] is
//! what stops that reaching a page, and the two checks are separate on purpose: a driver can be
//! correct and misconfigured, or fast and wrong, and one verdict covering both would hide whichever
//! of the two happened to be fine.
//!
//! [`Cold`]: crate::protocol::Cold

use crate::protocol::Report;
use crate::workload::Source;

/// How far a driver's ratio may sit from the shared machine factor. Twenty five percent, from the
/// gate in `docs/ROADMAP.md`.
pub const BAND: f64 = 0.25;

/// The day both files below were fetched.
const FETCHED: &str = "2026-09-06";

const DUCKDB_JSON: &str = include_str!("../leaderboard/duckdb-c6a.4xlarge.json");
const DATAFUSION_JSON: &str = include_str!("../leaderboard/datafusion-c6a.4xlarge.json");

/// Which published result a driver is measured against.
///
/// One entry per driver that has one, and the machine class is part of the name because upstream
/// publishes the same system on several and picking between them is a decision rather than a
/// detail. `c6a.4xlarge` is the class the leaderboard shows by default, so it is the one a reader
/// comparing our table to theirs will have in front of them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reference {
    /// `DuckDB` on `c6a.4xlarge`, untuned.
    DuckDb,
    /// `DataFusion` reading Parquet on `c6a.4xlarge`, untuned.
    DataFusion,
}

impl Reference {
    /// The file, verbatim.
    #[must_use]
    pub fn json(self) -> &'static str {
        match self {
            Self::DuckDb => DUCKDB_JSON,
            Self::DataFusion => DATAFUSION_JSON,
        }
    }

    /// Where the file came from and what it hashed to when it was fetched.
    #[must_use]
    pub fn source(self) -> Source {
        match self {
            Self::DuckDb => Source {
                url: "https://raw.githubusercontent.com/ClickHouse/ClickBench/main/duckdb/results/20260511/c6a.4xlarge.json"
                    .to_owned(),
                fetched: FETCHED.to_owned(),
                blake3: "e4ad8de923bf96e636944e0af84e25df277247f0936e54c80c23b3ab26fe5e5b".to_owned(),
            },
            Self::DataFusion => Source {
                url: "https://raw.githubusercontent.com/ClickHouse/ClickBench/main/datafusion/results/20260820/c6a.4xlarge.json"
                    .to_owned(),
                fetched: FETCHED.to_owned(),
                blake3: "b0a5b81ceddb0bd66ac1aaad70798f67cfc46e9bc6f1cec589a0c89cb3885c34".to_owned(),
            },
        }
    }

    /// The parsed file.
    ///
    /// # Panics
    ///
    /// If the carried file does not parse, which is a bug in this crate rather than something a
    /// caller can do anything about. The test below parses both.
    #[must_use]
    pub fn published(self) -> Published {
        serde_json::from_str(self.json()).expect("a carried leaderboard file has to parse")
    }
}

/// Which published result a driver is measured against, by the name the driver calls itself.
///
/// `arrow-parquet` has none and never will. It is a reader rather than an engine, it answers none
/// of the forty three, and a reference for it would be a reference for a number that does not
/// exist. `None` here is a comparison that says so rather than a driver quietly left out of the
/// table.
#[must_use]
pub fn reference(driver: &str) -> Option<Reference> {
    match driver {
        "duckdb" => Some(Reference::DuckDb),
        "datafusion" => Some(Reference::DataFusion),
        _ => None,
    }
}

/// A published `ClickBench` result, in the shape upstream stores one.
///
/// Only the fields this crate reads are named. Upstream carries several more and adds to them, and
/// a struct that refused to parse a file because it grew a field would be a gate that fails on
/// somebody else's release schedule.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Published {
    /// What upstream calls the system, such as `DataFusion (Parquet, single)`.
    pub system: String,
    /// The machine class, such as `c6a.4xlarge`.
    pub machine: String,
    /// The day upstream took the numbers, as `YYYY-MM-DD`.
    pub date: String,
    /// Load time in seconds, or `None` where upstream did not record one.
    pub load_time: Option<f64>,
    /// What the loaded corpus took on disk, in bytes.
    pub data_size: Option<u64>,
    /// Three runs per query in seconds, in query order, `None` for a query the system did not run.
    pub result: Vec<Vec<Option<f64>>>,
}

impl Published {
    /// The first run of a query, in seconds, which is the leaderboard's cold column.
    #[must_use]
    pub fn cold(&self, at: usize) -> Option<f64> {
        *self.result.get(at)?.first()?
    }

    /// The best of the runs after the first, in seconds, which is the leaderboard's hot column.
    #[must_use]
    pub fn hot(&self, at: usize) -> Option<f64> {
        self.result
            .get(at)?
            .get(1..)?
            .iter()
            .copied()
            .flatten()
            .reduce(f64::min)
    }

    /// How many queries the file carries.
    #[must_use]
    pub fn queries(&self) -> usize {
        self.result.len()
    }
}

/// Which column of the protocol a comparison is over.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    /// The first of the three runs.
    Cold,
    /// The best of the runs after the first.
    Hot,
}

impl Column {
    /// What it is called in a table.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Hot => "hot",
        }
    }
}

impl std::fmt::Display for Column {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name())
    }
}

/// One query on both sides of the comparison.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Point {
    /// Which query, such as `q23`.
    pub id: String,
    /// What it took here, in seconds.
    pub ours: f64,
    /// What it took upstream, in seconds.
    pub theirs: f64,
    /// This query's ratio divided by the driver's overall ratio.
    ///
    /// One means this query is as far from upstream as the driver is on the whole, which is what
    /// every query looks like when the only difference is the machine. A large value is a query
    /// that is slow here for a reason of its own, and naming that query is what turns a failed gate
    /// into an issue somebody can act on.
    pub relative: f64,
}

/// One driver next to the published numbers for the same system.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Standing {
    /// What the driver calls itself.
    pub driver: String,
    /// Which published result this is against.
    pub reference: Reference,
    /// What upstream calls the system there.
    pub system: String,
    /// The machine class upstream used.
    pub machine: String,
    /// Which column.
    pub column: Column,
    /// Every query with a usable number on both sides, worst first.
    pub points: Vec<Point>,
    /// Query ids with a number on one side only, or a number that cannot be divided by.
    pub skipped: Vec<String>,
    /// Our geometric mean over [`Standing::points`], in seconds.
    pub ours: f64,
    /// Upstream's geometric mean over the same queries, in seconds.
    pub theirs: f64,
    /// [`Standing::ours`] divided by [`Standing::theirs`].
    pub ratio: f64,
}

impl Standing {
    /// Puts one report next to one published result.
    ///
    /// Queries are matched by id rather than by position, because a run under a shuffled schedule
    /// records its queries in the order they ran and upstream's file is in the order the queries
    /// are published. Matching by position there would compare two different queries and produce a
    /// number that looks entirely reasonable.
    ///
    /// Returns `None` when nothing was comparable, which is a report where the driver answered
    /// nothing rather than a failure of this function.
    #[must_use]
    pub fn new(report: &Report, reference: Reference, column: Column) -> Option<Self> {
        let published = reference.published();
        let mut pairs = Vec::new();
        let mut skipped = Vec::new();
        for query in &report.queries {
            let at = index(&query.id);
            let ours = match column {
                Column::Cold => query.cold(),
                Column::Hot => query.hot(),
            }
            .map(|nanoseconds| nanoseconds / 1e9);
            let theirs = at.and_then(|at| match column {
                Column::Cold => published.cold(at),
                Column::Hot => published.hot(at),
            });
            match (ours, theirs) {
                (Some(ours), Some(theirs)) if ours > 0.0 && theirs > 0.0 => {
                    pairs.push((query.id.clone(), ours, theirs));
                }
                _ => skipped.push(query.id.clone()),
            }
        }
        let ours = geomean(pairs.iter().map(|&(_, ours, _)| ours))?;
        let theirs = geomean(pairs.iter().map(|&(_, _, theirs)| theirs))?;
        let ratio = ours / theirs;
        let mut points: Vec<Point> = pairs
            .into_iter()
            .map(|(id, ours, theirs)| Point {
                id,
                ours,
                theirs,
                relative: (ours / theirs) / ratio,
            })
            .collect();
        points.sort_by(|left, right| {
            right
                .relative
                .partial_cmp(&left.relative)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Some(Self {
            driver: report.driver.clone(),
            reference,
            system: published.system,
            machine: published.machine,
            column,
            points,
            skipped,
            ours,
            theirs,
            ratio,
        })
    }
}

/// Every driver in one run next to the published numbers, and the machine factor they imply.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Calibration {
    /// Which column.
    pub column: Column,
    /// One per driver that had a published result to compare against.
    pub standings: Vec<Standing>,
    /// Drivers with no published result, by the name they call themselves.
    pub unmatched: Vec<String>,
    /// The geometric mean of the standings' ratios, which is this machine against upstream's.
    pub machine: f64,
}

impl Calibration {
    /// Compares a set of reports, one per driver, against the published numbers.
    ///
    /// Returns `None` when no report had a reference to compare against, because a machine factor
    /// estimated from nothing is not a number anybody should be shown.
    #[must_use]
    pub fn new(reports: &[&Report], column: Column) -> Option<Self> {
        let mut standings = Vec::new();
        let mut unmatched = Vec::new();
        for report in reports {
            match reference(&report.driver).and_then(|it| Standing::new(report, it, column)) {
                Some(standing) => standings.push(standing),
                None => unmatched.push(report.driver.clone()),
            }
        }
        let machine = geomean(standings.iter().map(|standing| standing.ratio))?;
        Some(Self {
            column,
            standings,
            unmatched,
            machine,
        })
    }

    /// How far one driver sits from the shared machine factor, as a fraction of it.
    ///
    /// Zero is a driver exactly as far from upstream as the rest of the run. The sign says which
    /// way, and a positive number is the slow one.
    #[must_use]
    pub fn drift(&self, standing: &Standing) -> f64 {
        standing.ratio / self.machine - 1.0
    }

    /// Whether one driver is inside [`BAND`].
    #[must_use]
    pub fn inside(&self, standing: &Standing) -> bool {
        self.drift(standing).abs() <= BAND
    }

    /// The drivers outside the band. Each of these is a misconfiguration issue.
    #[must_use]
    pub fn outside(&self) -> Vec<&Standing> {
        self.standings
            .iter()
            .filter(|standing| !self.inside(standing))
            .collect()
    }

    /// Whether every compared driver is inside the band.
    ///
    /// Says nothing about [`Calibration::unmatched`]. A driver with no published result is not a
    /// failure and it is not a pass either, and the caller reports it rather than folding it into
    /// a verdict.
    #[must_use]
    pub fn clean(&self) -> bool {
        self.outside().is_empty()
    }
}

/// The geometric mean, which is the statistic `ClickBench` aggregates its queries with.
///
/// Taken in log space, because forty three durations multiplied together leave the range of an
/// `f64` long before the root is taken.
///
/// `None` for an empty set, which is not a zero, and `None` for any value that is not above zero. A
/// duration of zero sends its logarithm to negative infinity and the mean to zero, which reads as an
/// infinitely fast system rather than as the stopped clock it actually is.
#[must_use]
pub fn geomean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0_u32;
    for value in values {
        if value <= 0.0 {
            return None;
        }
        sum += value.ln();
        count += 1;
    }
    (count > 0).then(|| (sum / f64::from(count)).exp())
}

/// The position of a query in the published file, from an id like `q23`.
fn index(id: &str) -> Option<usize> {
    id.strip_prefix('q')?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clickbench::QUERIES;
    use crate::digest::Digest;
    use crate::protocol::{Cold, Measured, Outcome, Run};
    use crate::workload::Source;
    use bench_driver::Answer;

    const ALL: [Reference; 2] = [Reference::DuckDb, Reference::DataFusion];

    #[test]
    fn the_carried_files_are_the_files_that_were_fetched() {
        let drifted: Vec<String> = ALL
            .into_iter()
            .filter_map(|reference| {
                let digest = blake3::hash(reference.json().as_bytes())
                    .to_hex()
                    .to_string();
                (digest != reference.source().blake3).then(|| {
                    format!(
                        "the {:?} file hashes to {digest} and the source says {}",
                        reference,
                        reference.source().blake3,
                    )
                })
            })
            .collect();
        assert!(drifted.is_empty(), "{}", drifted.join("\n"));
    }

    #[test]
    fn both_published_files_carry_all_forty_three() {
        for reference in ALL {
            let published = reference.published();
            assert_eq!(published.queries(), QUERIES, "{reference:?}");
            assert_eq!(published.machine, "c6a.4xlarge", "{reference:?}");
        }
    }

    #[test]
    fn the_hot_column_is_the_better_of_the_runs_after_the_first() {
        let published = Published {
            system: "T".to_owned(),
            machine: "m".to_owned(),
            date: "2026-01-01".to_owned(),
            load_time: None,
            data_size: None,
            result: vec![vec![Some(9.0), Some(3.0), Some(2.0)]],
        };
        assert_eq!(published.cold(0), Some(9.0));
        assert_eq!(published.hot(0), Some(2.0));
        assert_eq!(published.cold(1), None);
    }

    #[test]
    fn a_query_upstream_could_not_run_has_no_number_rather_than_a_zero() {
        let published = Published {
            system: "T".to_owned(),
            machine: "m".to_owned(),
            date: "2026-01-01".to_owned(),
            load_time: None,
            data_size: None,
            result: vec![vec![None, None, None]],
        };
        assert_eq!(published.cold(0), None);
        assert_eq!(published.hot(0), None);
    }

    #[test]
    fn a_driver_with_no_published_result_is_named_rather_than_dropped() {
        assert_eq!(reference("duckdb"), Some(Reference::DuckDb));
        assert_eq!(reference("datafusion"), Some(Reference::DataFusion));
        assert_eq!(reference("arrow-parquet"), None);
        assert_eq!(reference("something-new"), None);
    }

    #[test]
    fn the_geometric_mean_is_the_geometric_mean() {
        let mean = geomean([1.0, 4.0, 16.0].into_iter()).expect("three values have a mean");
        assert!((mean - 4.0).abs() < 1e-9, "{mean}");
    }

    #[test]
    fn nothing_to_average_is_not_a_zero() {
        assert_eq!(geomean(std::iter::empty()), None);
        assert_eq!(geomean([1.0, 0.0].into_iter()), None);
    }

    /// A stand in digest. Nothing here compares answers, so one value for every run is right.
    fn nothing() -> Digest {
        Digest::of(&Answer {
            rows: 0,
            columns: 0,
            body: String::new(),
        })
    }

    /// A report where every query took `seconds`, so the arithmetic below is checkable by hand.
    fn flat(driver: &str, seconds: f64) -> Report {
        Report {
            driver: driver.to_owned(),
            version: "test".to_owned(),
            workload: "clickbench".to_owned(),
            source: Source {
                url: "https://example.invalid".to_owned(),
                fetched: "2026-01-01".to_owned(),
                blake3: "0".repeat(64),
            },
            queries: (0..QUERIES)
                .map(|number| Measured {
                    id: format!("q{number}"),
                    cache: Cold::Dropped,
                    outcome: Outcome::Answered {
                        runs: (0..3)
                            .map(|_| Run {
                                nanoseconds: seconds * 1e9,
                                rows: 1,
                                digest: nothing(),
                            })
                            .collect(),
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn a_driver_as_far_out_as_the_rest_of_the_run_is_the_machine_and_not_a_mistake() {
        // Both drivers exactly ten times upstream, which is a slow machine and nothing else.
        let duckdb = scaled("duckdb", Reference::DuckDb, 10.0);
        let datafusion = scaled("datafusion", Reference::DataFusion, 10.0);
        let calibration = Calibration::new(&[&duckdb, &datafusion], Column::Hot)
            .expect("two drivers have references");
        assert!(
            (calibration.machine - 10.0).abs() < 1e-9,
            "{:?}",
            calibration.machine
        );
        assert!(calibration.clean());
        for standing in &calibration.standings {
            assert!(calibration.drift(standing).abs() < 1e-9, "{standing:?}");
        }
    }

    #[test]
    fn one_driver_out_of_line_with_the_other_is_the_failure_this_exists_to_find() {
        let duckdb = scaled("duckdb", Reference::DuckDb, 10.0);
        let datafusion = scaled("datafusion", Reference::DataFusion, 40.0);
        let calibration = Calibration::new(&[&duckdb, &datafusion], Column::Hot)
            .expect("two drivers have references");
        assert!(!calibration.clean());
        let outside: Vec<&str> = calibration
            .outside()
            .iter()
            .map(|standing| standing.driver.as_str())
            .collect();
        // Both are named, because with two drivers there is no third opinion about which of them
        // moved. Which one is wrong is a decision for whoever reads the issue.
        assert_eq!(outside, ["duckdb", "datafusion"]);
    }

    #[test]
    fn a_reader_that_answered_nothing_is_reported_rather_than_compared() {
        let duckdb = scaled("duckdb", Reference::DuckDb, 10.0);
        let datafusion = scaled("datafusion", Reference::DataFusion, 10.0);
        let mut reader = flat("arrow-parquet", 1.0);
        for query in &mut reader.queries {
            query.outcome = Outcome::Unsupported {
                why: "this is a reader".to_owned(),
            };
        }
        let calibration = Calibration::new(&[&duckdb, &datafusion, &reader], Column::Hot)
            .expect("two of the three have references");
        assert_eq!(calibration.standings.len(), 2);
        assert_eq!(calibration.unmatched, ["arrow-parquet"]);
        assert!(calibration.clean());
    }

    #[test]
    fn one_slow_query_is_named_even_when_the_driver_is_in_band() {
        let mut report = scaled("duckdb", Reference::DuckDb, 10.0);
        // q17 alone made a hundred times slower, which is what a missing index or a spilled hash
        // table looks like on one query and on nothing else.
        let published = Reference::DuckDb.published();
        let at = 17;
        let hot = published.hot(at).expect("q17 has a published number");
        for query in &mut report.queries {
            if query.id == "q17" {
                query.outcome = Outcome::Answered {
                    runs: (0..3)
                        .map(|_| Run {
                            nanoseconds: hot * 1000.0 * 1e9,
                            rows: 1,
                            digest: nothing(),
                        })
                        .collect(),
                };
            }
        }
        let standing =
            Standing::new(&report, Reference::DuckDb, Column::Hot).expect("the driver answered");
        assert_eq!(standing.points[0].id, "q17");
        assert!(
            standing.points[0].relative > 10.0,
            "{:?}",
            standing.points[0]
        );
    }

    #[test]
    fn queries_are_matched_by_id_and_not_by_the_order_they_ran_in() {
        let ordered = scaled("duckdb", Reference::DuckDb, 10.0);
        let mut shuffled = ordered.clone();
        shuffled.queries.reverse();
        let left = Standing::new(&ordered, Reference::DuckDb, Column::Hot).expect("answered");
        let right = Standing::new(&shuffled, Reference::DuckDb, Column::Hot).expect("answered");
        assert!(
            (left.ratio - right.ratio).abs() < 1e-9,
            "{} {}",
            left.ratio,
            right.ratio
        );
    }

    /// A report that is `factor` times the published numbers on every query, so the ratio it
    /// produces is exactly `factor` whatever the published file happens to say.
    fn scaled(driver: &str, reference: Reference, factor: f64) -> Report {
        let published = reference.published();
        let mut report = flat(driver, 1.0);
        for (at, query) in report.queries.iter_mut().enumerate() {
            let hot = published
                .hot(at)
                .expect("the published file has every query");
            let cold = published
                .cold(at)
                .expect("the published file has every query");
            query.outcome = Outcome::Answered {
                runs: [cold, hot, hot]
                    .into_iter()
                    .map(|seconds| Run {
                        nanoseconds: seconds * factor * 1e9,
                        rows: 1,
                        digest: nothing(),
                    })
                    .collect(),
            };
        }
        report
    }
}
