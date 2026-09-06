//! Three runs per query, cold and hot reported separately.
//!
//! This is `ClickBench`'s protocol rather than one invented here, and that is the point of it.
//! Numbers produced under a different protocol cannot be placed next to the public leaderboard, and
//! being able to place them there is most of the reason for running the thing at all.
//!
//! Each query is run three times in a row. The page cache is dropped before the three, not between
//! them, so the first run reads from disk and the second and third read from whatever the machine
//! is now holding. The leaderboard's cold column is the first run and its hot column is the better
//! of the second and third. That is [`Measured::cold`] and [`Measured::hot`].
//!
//! # What cold means, and where it stops meaning it
//!
//! Dropping the page cache needs root on Linux and has no equivalent on some machines. When the
//! cache was not actually dropped the first run is a warm number under a cold heading, which is the
//! single most misleading row a benchmark can print, so it is labelled: [`Cold::Kept`] carries the
//! reason and travels in the result. Dropping the cache is the runner's job, not this module's,
//! because it is a privileged operation on a machine and this module is a description of a
//! protocol. This module asks for it through [`PageCache`] and records the answer it gets.
//!
//! Even a real drop is only cold for a system that reads the corpus off the filesystem. A system
//! that ingested the data into its own process and is holding pages in a buffer pool of its own is
//! not cold in any sense after the drop, and no page cache call can make it so. Upstream has the
//! same problem and the same answer: the load phase is measured separately and a reader who cares
//! looks at both. It is written here so that nobody reads the cold column as if it meant one thing
//! across the whole table.
//!
//! # A query that a system cannot express
//!
//! It is recorded as unsupported and the workload carries on to the next query. It is not a
//! failure, and above all it is not a fast time against a query nobody asked. The reference reader
//! answers none of the forty three, which is what a reader rather than an engine looks like from
//! here, and forty three unsupported rows say that plainly.

use bench_driver::{DriverError, Session};

use crate::digest::Digest;
use crate::workload::{Source, Workload};

/// How many times each query is run. Three, because that is what upstream does.
pub const RUNS: usize = 3;

/// Whatever can drop what the operating system is holding.
///
/// Implemented by the runner, which is where privileged operations on a machine belong. This
/// module only asks and records the answer.
pub trait PageCache {
    /// Drops the page cache, and says whether it managed to.
    ///
    /// An implementation that cannot must return [`Cold::Kept`] with a reason rather than
    /// pretending, because the caller has no other way to find out and the first run of every query
    /// after this call is going to be published under the word cold.
    fn evict(&mut self) -> Cold;
}

/// A page cache that is never dropped, and says so.
///
/// For a machine where the call needs a privilege this process does not have, and for tests. The
/// numbers it produces are still worth having as long as nothing labels them cold, which is exactly
/// what [`Cold::Kept`] stops happening.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Warm;

impl PageCache for Warm {
    fn evict(&mut self) -> Cold {
        Cold::Kept {
            why: "nothing was asked to drop the page cache on this run".to_owned(),
        }
    }
}

/// Whether the first of a query's three runs was really cold.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cold {
    /// The page cache was dropped before the three runs.
    Dropped,
    /// It was not, and this is why. The first run is a warm number and has to be read as one.
    Kept {
        /// What stopped it.
        why: String,
    },
}

impl Cold {
    /// Whether the first run can be published under the word cold.
    #[must_use]
    pub fn is_cold(&self) -> bool {
        matches!(self, Self::Dropped)
    }
}

/// One of a query's three runs.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Run {
    /// What it took, in nanoseconds, as the session timed it from outside the driver.
    pub nanoseconds: f64,
    /// How many rows came back.
    pub rows: u64,
    /// What the answer hashed to.
    pub digest: Digest,
}

/// What happened when a system was asked one query three times.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// It answered. There are [`RUNS`] of these unless a later run failed after an earlier one
    /// succeeded, which is itself worth seeing.
    Answered {
        /// The runs, in the order they happened.
        runs: Vec<Run>,
    },
    /// It cannot express this query. Not a failure, and not a fast time either.
    Unsupported {
        /// Why not, in the driver's own words.
        why: String,
    },
    /// It tried and something went wrong.
    Failed {
        /// What went wrong, in the driver's own words.
        why: String,
    },
}

/// One query, run under the protocol.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Measured {
    /// Which query, such as `q23`.
    pub id: String,
    /// Whether the page cache was really dropped before the first run.
    pub cache: Cold,
    /// What happened.
    pub outcome: Outcome,
}

impl Measured {
    /// The first run, in nanoseconds, which is the leaderboard's cold column.
    ///
    /// Worth reading next to [`Measured::cache`], which says whether the word applies.
    #[must_use]
    pub fn cold(&self) -> Option<f64> {
        self.runs().first().map(|run| run.nanoseconds)
    }

    /// The best of the runs after the first, in nanoseconds, which is the leaderboard's hot column.
    #[must_use]
    pub fn hot(&self) -> Option<f64> {
        self.runs()
            .get(1..)?
            .iter()
            .map(|run| run.nanoseconds)
            .reduce(f64::min)
    }

    /// What the answer hashed to, from the first run.
    #[must_use]
    pub fn digest(&self) -> Option<Digest> {
        self.runs().first().map(|run| run.digest)
    }

    /// Whether the three runs produced the same answer as each other.
    ///
    /// A system that disagreed with itself between two runs of one query is a finding on its own,
    /// separate from two systems disagreeing with each other, and a comparison that only ever looks
    /// at the first run would never see it. `ClickBench` has queries where this is expected rather
    /// than alarming, such as a group by with a limit and no order by, and the point of recording
    /// it is that somebody gets to decide which kind it is.
    #[must_use]
    pub fn steady(&self) -> bool {
        let runs = self.runs();
        match runs.first() {
            None => true,
            Some(first) => runs.iter().all(|run| run.digest == first.digest),
        }
    }

    /// Whether the system answered at all.
    #[must_use]
    pub fn answered(&self) -> bool {
        matches!(self.outcome, Outcome::Answered { .. })
    }

    fn runs(&self) -> &[Run] {
        match &self.outcome {
            Outcome::Answered { runs } => runs,
            Outcome::Unsupported { .. } | Outcome::Failed { .. } => &[],
        }
    }
}

/// What one system did on one workload.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Report {
    /// What the system calls itself.
    pub driver: String,
    /// What version of it this was, as it reported itself.
    pub version: String,
    /// Which workload.
    pub workload: String,
    /// Where the queries came from.
    pub source: Source,
    /// Every query, in the order the workload publishes them.
    pub queries: Vec<Measured>,
}

impl Report {
    /// How many queries the system answered.
    #[must_use]
    pub fn answered(&self) -> usize {
        self.queries.iter().filter(|query| query.answered()).count()
    }
}

/// Runs a whole workload against one prepared and loaded system.
///
/// The caller prepares the system and loads the table first, because both need paths and a machine
/// class that this module has no business knowing. What this function owns is the part of the
/// protocol that has to be identical for every system: three runs, the cache dropped before each
/// query's three, and a query the system cannot answer recorded rather than fatal.
///
/// It never returns an error. A driver that fails on one query has failed on one query, and a
/// workload that stopped there would throw away the forty two answers around it.
pub fn measure(
    session: &mut Session<'_>,
    workload: &Workload,
    cache: &mut dyn PageCache,
) -> Report {
    let mut queries = Vec::with_capacity(workload.queries.len());
    for query in &workload.queries {
        let cold = cache.evict();
        let mut runs = Vec::with_capacity(RUNS);
        let mut outcome = None;
        for _ in 0..RUNS {
            match session.query(query) {
                Ok((answer, nanoseconds)) => runs.push(Run {
                    nanoseconds,
                    rows: answer.rows,
                    digest: Digest::of(&answer),
                }),
                // The first run to go wrong decides the outcome, and the runs before it are
                // dropped rather than reported: two timings under a heading that says three is a
                // row nobody can compare with anything.
                Err(DriverError::Unsupported { why, .. }) => {
                    outcome = Some(Outcome::Unsupported { why });
                    break;
                }
                Err(error) => {
                    outcome = Some(Outcome::Failed {
                        why: error.to_string(),
                    });
                    break;
                }
            }
        }
        queries.push(Measured {
            id: query.id.clone(),
            cache: cold,
            outcome: outcome.unwrap_or(Outcome::Answered { runs }),
        });
    }

    Report {
        driver: session.name().to_owned(),
        version: session.version(),
        workload: workload.name.to_owned(),
        source: workload.source.clone(),
        queries,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bench_driver::{Answer, Driver, Format, Load, Query, Rows, Setup, Value};

    use super::*;

    /// A system that answers a fixed number of queries and has an opinion about the rest.
    struct Toy {
        /// How many times run has been called, which is what the answers vary with.
        calls: i64,
        /// Whether the answer changes between runs of the same query.
        drifts: bool,
        /// Which query id it refuses, and how.
        refuses: Option<(&'static str, bool)>,
    }

    impl Driver for Toy {
        fn name(&self) -> &'static str {
            "toy"
        }

        fn version(&self) -> String {
            "1".to_owned()
        }

        fn prepare(&mut self, _setup: &Setup) -> Result<(), DriverError> {
            Ok(())
        }

        fn load(&mut self, _load: &Load) -> Result<(), DriverError> {
            Ok(())
        }

        fn run(&mut self, query: &Query) -> Result<Answer, DriverError> {
            self.calls += 1;
            if let Some((id, unsupported)) = self.refuses
                && query.id == id
            {
                return Err(if unsupported {
                    DriverError::unsupported(id, "a toy cannot do that")
                } else {
                    DriverError::system(id, std::io::Error::other("the toy fell over"))
                });
            }
            let mut rows = Rows::new();
            rows.push([Value::Int(if self.drifts { self.calls } else { 1 })])?;
            Ok(rows.finish(true))
        }
    }

    /// A page cache that says it dropped, so the labelling can be tested both ways.
    #[derive(Default)]
    struct Dropping {
        times: usize,
    }

    impl PageCache for Dropping {
        fn evict(&mut self) -> Cold {
            self.times += 1;
            Cold::Dropped
        }
    }

    fn toy(refuses: Option<(&'static str, bool)>, drifts: bool) -> Toy {
        Toy {
            calls: 0,
            drifts,
            refuses,
        }
    }

    fn workload(count: usize) -> Workload {
        Workload {
            name: "toy",
            corpus: "none",
            table: "hits",
            format: Format::Parquet,
            source: Source {
                url: "https://example.invalid/queries.sql".to_owned(),
                fetched: "2026-09-06".to_owned(),
                blake3: "0".repeat(64),
            },
            queries: (0..count)
                .map(|number| Query {
                    id: format!("q{number}"),
                    sql: "SELECT 1 FROM hits".to_owned(),
                    ordered: true,
                })
                .collect(),
        }
    }

    fn run(driver: &mut Toy, count: usize, cache: &mut dyn PageCache) -> Report {
        let mut session = Session::new(driver);
        session
            .prepare(&Setup {
                directory: PathBuf::from("scratch"),
                threads: 1,
                memory: 1 << 30,
            })
            .unwrap();
        session
            .load(&Load {
                table: "hits".to_owned(),
                files: vec![PathBuf::from("hits.parquet")],
                format: Format::Parquet,
            })
            .unwrap();
        measure(&mut session, &workload(count), cache)
    }

    #[test]
    fn every_query_is_run_three_times_with_the_cache_dropped_before_each_three() {
        let mut driver = toy(None, false);
        let mut cache = Dropping::default();
        let report = run(&mut driver, 4, &mut cache);

        assert_eq!(report.queries.len(), 4);
        assert_eq!(report.answered(), 4);
        assert_eq!(driver.calls, 12);
        // Once per query, not once per run. Dropping between the three would make every run cold
        // and there would be no hot column at all.
        assert_eq!(cache.times, 4);
        for query in &report.queries {
            assert!(matches!(&query.outcome, Outcome::Answered { runs } if runs.len() == RUNS));
            assert!(query.cache.is_cold());
        }
    }

    #[test]
    fn the_cold_number_is_the_first_run_and_the_hot_one_is_the_best_of_the_others() {
        let mut driver = toy(None, false);
        let report = run(&mut driver, 1, &mut Warm);
        let query = &report.queries[0];
        let Outcome::Answered { runs } = &query.outcome else {
            panic!("the toy answers");
        };

        assert_eq!(query.cold(), Some(runs[0].nanoseconds));
        let best = runs[1].nanoseconds.min(runs[2].nanoseconds);
        assert_eq!(query.hot(), Some(best));
    }

    #[test]
    fn a_run_that_could_not_drop_the_cache_says_so_rather_than_calling_itself_cold() {
        let mut driver = toy(None, false);
        let report = run(&mut driver, 1, &mut Warm);
        let cache = &report.queries[0].cache;

        assert!(!cache.is_cold());
        let Cold::Kept { why } = cache else {
            panic!("Warm keeps the cache");
        };
        assert!(!why.is_empty());
    }

    #[test]
    fn a_query_the_system_cannot_express_is_recorded_and_the_rest_still_run() {
        let mut driver = toy(Some(("q1", true)), false);
        let report = run(&mut driver, 3, &mut Warm);

        assert_eq!(report.queries.len(), 3);
        assert_eq!(report.answered(), 2);
        assert!(matches!(
            &report.queries[1].outcome,
            Outcome::Unsupported { why } if why.contains("toy")
        ));
        // And it has no timing at all, rather than a fast one.
        assert_eq!(report.queries[1].cold(), None);
        assert_eq!(report.queries[1].hot(), None);
    }

    #[test]
    fn a_query_that_failed_is_not_recorded_as_one_the_system_cannot_express() {
        // The difference matters. Unsupported is a fact about the system and a failure is a fact
        // about this run, and a table that merged them would let a broken run read as a limitation.
        let mut driver = toy(Some(("q0", false)), false);
        let report = run(&mut driver, 1, &mut Warm);

        assert!(matches!(&report.queries[0].outcome, Outcome::Failed { .. }));
    }

    #[test]
    fn a_system_that_answered_differently_on_the_second_run_is_not_steady() {
        let mut driver = toy(None, true);
        let report = run(&mut driver, 1, &mut Warm);

        assert!(report.queries[0].answered());
        assert!(!report.queries[0].steady());

        let mut driver = toy(None, false);
        let report = run(&mut driver, 1, &mut Warm);
        assert!(report.queries[0].steady());
    }

    #[test]
    fn the_report_carries_what_the_system_calls_itself_and_where_the_queries_came_from() {
        let mut driver = toy(None, false);
        let report = run(&mut driver, 1, &mut Warm);

        assert_eq!(report.driver, "toy");
        assert_eq!(report.version, "1");
        assert_eq!(report.workload, "toy");
        assert_eq!(report.source.fetched, "2026-09-06");
    }
}
