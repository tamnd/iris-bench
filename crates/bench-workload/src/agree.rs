//! Whether the systems agreed about the answers.
//!
//! Timings are worth nothing until this passes. Two systems that disagree about what a query
//! returns are not two systems whose speeds can be compared, they are one correct system and one
//! that is fast because it is doing less, and which is which is not decidable from the times.
//!
//! The comparison is between digests of the canonical rendering, so it is already blind to the
//! things two engines are allowed to differ on: row order where the query named none, and the last
//! digits of a float. `bench_driver::answer` documents what those two rules cost. Everything that
//! survives them is a real difference and shows up here.

use crate::digest::Digest;
use crate::protocol::Report;

/// What one system said about one query.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Reading {
    /// What the system calls itself.
    pub driver: String,
    /// What its answer hashed to.
    pub digest: Digest,
    /// How many rows it returned, so that a reader has something to look at before opening the
    /// results themselves.
    pub rows: u64,
}

/// A query the systems did not agree about.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Disagreement {
    /// Which query.
    pub query: String,
    /// What each system that answered said, in the order the reports were given.
    pub readings: Vec<Reading>,
}

/// A system that disagreed with itself between two runs of one query.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Unstable {
    /// Which system.
    pub driver: String,
    /// Which query.
    pub query: String,
}

/// What comparing a set of reports found.
///
/// Every query lands in exactly one of the first four lists, so the four add up to the workload and
/// nothing is quietly left out of the arithmetic. [`Comparison::unstable`] is separate because it
/// is about one system rather than about two, and a query can be both unstable and agreed.
#[derive(Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Comparison {
    /// Queries two or more systems answered identically.
    pub agreed: Vec<String>,
    /// Queries two or more systems answered differently.
    pub disagreed: Vec<Disagreement>,
    /// Queries exactly one system answered, so nothing checked it.
    ///
    /// Not the same as agreement, and a table that counted it as agreement would report the
    /// reference reader's forty three unsupported rows as forty three queries confirmed by
    /// somebody.
    pub alone: Vec<String>,
    /// Queries no system answered.
    pub unanswered: Vec<String>,
    /// Systems that answered one query two different ways in the same three runs.
    pub unstable: Vec<Unstable>,
}

impl Comparison {
    /// Whether anything here should stop a set of timings being published.
    #[must_use]
    pub fn clean(&self) -> bool {
        self.disagreed.is_empty() && self.unstable.is_empty()
    }
}

/// Compares what every system said, query by query.
///
/// Queries come out in the order the first report lists them, with anything only later reports have
/// after that, so the output reads in workload order rather than in whatever order a map happened
/// to hold.
#[must_use]
pub fn compare(reports: &[Report]) -> Comparison {
    let mut comparison = Comparison::default();
    for id in ids(reports) {
        let mut readings = Vec::new();
        for report in reports {
            let Some(measured) = report.queries.iter().find(|query| query.id == id) else {
                continue;
            };
            if !measured.steady() {
                comparison.unstable.push(Unstable {
                    driver: report.driver.clone(),
                    query: id.clone(),
                });
            }
            let (Some(digest), Some(rows)) = (measured.digest(), rows(measured)) else {
                continue;
            };
            readings.push(Reading {
                driver: report.driver.clone(),
                digest,
                rows,
            });
        }

        match readings.len() {
            0 => comparison.unanswered.push(id),
            1 => comparison.alone.push(id),
            _ => {
                let first = readings[0].digest;
                if readings.iter().all(|reading| reading.digest == first) {
                    comparison.agreed.push(id);
                } else {
                    comparison.disagreed.push(Disagreement {
                        query: id,
                        readings,
                    });
                }
            }
        }
    }
    comparison
}

/// Every query id any report mentions, in the order they are first mentioned.
fn ids(reports: &[Report]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for report in reports {
        for query in &report.queries {
            if !ids.iter().any(|seen| seen == &query.id) {
                ids.push(query.id.clone());
            }
        }
    }
    ids
}

/// How many rows the first run returned, if it returned any.
fn rows(measured: &crate::protocol::Measured) -> Option<u64> {
    match &measured.outcome {
        crate::protocol::Outcome::Answered { runs } => runs.first().map(|run| run.rows),
        crate::protocol::Outcome::Unsupported { .. } | crate::protocol::Outcome::Failed { .. } => {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use bench_driver::{Answer, Rows, Value};

    use super::*;
    use crate::protocol::{Measured, Outcome, Run};
    use crate::workload::Source;

    fn digest(value: i64) -> Digest {
        let mut rows = Rows::new();
        rows.push([Value::Int(value)]).unwrap();
        Digest::of(&rows.finish(true))
    }

    fn answered(id: &str, digests: &[i64]) -> Measured {
        Measured {
            id: id.to_owned(),
            cache: crate::protocol::Cold::Dropped,
            outcome: Outcome::Answered {
                runs: digests
                    .iter()
                    .map(|value| Run {
                        nanoseconds: 1.0,
                        rows: 1,
                        digest: digest(*value),
                    })
                    .collect(),
            },
        }
    }

    fn unsupported(id: &str) -> Measured {
        Measured {
            id: id.to_owned(),
            cache: crate::protocol::Cold::Dropped,
            outcome: Outcome::Unsupported {
                why: "a reader is not an engine".to_owned(),
            },
        }
    }

    fn report(driver: &str, queries: Vec<Measured>) -> Report {
        Report {
            driver: driver.to_owned(),
            version: "1".to_owned(),
            workload: "clickbench".to_owned(),
            source: Source {
                url: "https://example.invalid/queries.sql".to_owned(),
                fetched: "2026-09-06".to_owned(),
                blake3: "0".repeat(64),
            },
            queries,
        }
    }

    #[test]
    fn two_systems_that_said_the_same_thing_agree() {
        let comparison = compare(&[
            report("one", vec![answered("q0", &[1, 1, 1])]),
            report("two", vec![answered("q0", &[1, 1, 1])]),
        ]);

        assert_eq!(comparison.agreed, ["q0"]);
        assert!(comparison.clean());
    }

    #[test]
    fn two_systems_that_said_different_things_are_reported_with_what_each_said() {
        let comparison = compare(&[
            report("one", vec![answered("q0", &[1, 1, 1])]),
            report("two", vec![answered("q0", &[2, 2, 2])]),
        ]);

        assert!(comparison.agreed.is_empty());
        assert_eq!(comparison.disagreed.len(), 1);
        assert_eq!(comparison.disagreed[0].query, "q0");
        let said: Vec<&str> = comparison.disagreed[0]
            .readings
            .iter()
            .map(|reading| reading.driver.as_str())
            .collect();
        assert_eq!(said, ["one", "two"]);
        assert_ne!(
            comparison.disagreed[0].readings[0].digest,
            comparison.disagreed[0].readings[1].digest
        );
        assert!(!comparison.clean());
    }

    #[test]
    fn a_query_only_one_system_answered_is_not_counted_as_confirmed() {
        // The reference reader answers none of ClickBench, so without this every query would be
        // agreed between two systems and the third would vanish from the arithmetic.
        let comparison = compare(&[
            report("one", vec![answered("q0", &[1, 1, 1])]),
            report("reader", vec![unsupported("q0")]),
        ]);

        assert!(comparison.agreed.is_empty());
        assert_eq!(comparison.alone, ["q0"]);
        assert!(comparison.clean());
    }

    #[test]
    fn a_query_nobody_answered_is_still_in_the_arithmetic() {
        let comparison = compare(&[
            report("one", vec![unsupported("q0")]),
            report("two", vec![unsupported("q0")]),
        ]);

        assert_eq!(comparison.unanswered, ["q0"]);
        let counted = comparison.agreed.len()
            + comparison.disagreed.len()
            + comparison.alone.len()
            + comparison.unanswered.len();
        assert_eq!(counted, 1);
    }

    #[test]
    fn a_system_that_disagreed_with_itself_is_named_even_though_the_two_systems_agreed() {
        let comparison = compare(&[
            report("one", vec![answered("q0", &[1, 2, 1])]),
            report("two", vec![answered("q0", &[1, 1, 1])]),
        ]);

        assert_eq!(comparison.agreed, ["q0"]);
        assert_eq!(comparison.unstable.len(), 1);
        assert_eq!(comparison.unstable[0].driver, "one");
        assert_eq!(comparison.unstable[0].query, "q0");
        assert!(!comparison.clean());
    }

    #[test]
    fn the_queries_come_out_in_workload_order() {
        let comparison = compare(&[
            report(
                "one",
                vec![
                    answered("q0", &[1]),
                    answered("q1", &[1]),
                    answered("q2", &[1]),
                ],
            ),
            report("two", vec![answered("q2", &[1]), answered("q0", &[1])]),
        ]);

        assert_eq!(comparison.agreed, ["q0", "q2"]);
        assert_eq!(comparison.alone, ["q1"]);
    }

    #[test]
    fn a_wider_answer_with_the_same_text_in_it_is_a_disagreement() {
        // Belt and braces on the digest covering the shape. This is the case a body only digest
        // would call agreement.
        let empty = |columns| Measured {
            id: "q0".to_owned(),
            cache: crate::protocol::Cold::Dropped,
            outcome: Outcome::Answered {
                runs: vec![Run {
                    nanoseconds: 1.0,
                    rows: 0,
                    digest: Digest::of(&Answer {
                        rows: 0,
                        columns,
                        body: String::new(),
                    }),
                }],
            },
        };
        let comparison = compare(&[report("one", vec![empty(1)]), report("two", vec![empty(2)])]);

        assert_eq!(comparison.disagreed.len(), 1);
    }
}
