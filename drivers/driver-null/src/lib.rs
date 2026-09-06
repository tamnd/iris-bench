//! A driver that does nothing.
//!
//! Running the harness against a system that does no work measures what the harness itself costs.
//! The instrumentation budget is one percent, and this is how it is checked.
//!
//! It is not a system under test and it never appears in a results table. Its answers are empty, so
//! every result digest it produces disagrees with every real system, which is the correct outcome
//! for a driver that did not compute anything.
//!
//! What it does record is what it was asked to do, so that a test can assert the harness called the
//! phases in the right order with the right arguments without needing a database installed.

use std::path::PathBuf;

use bench_driver::{Answer, Driver, DriverError, Load, Query, Rows, Setup};

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A system that does nothing, as fast as nothing can be done.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Null {
    /// The scratch directory preparation was given, once it has been.
    pub directory: Option<PathBuf>,
    /// The tables it was asked to load, in order.
    pub tables: Vec<String>,
    /// The query ids it was asked to answer, in order.
    pub queries: Vec<String>,
}

impl Null {
    /// A driver that has not been asked to do anything yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Driver for Null {
    fn name(&self) -> &'static str {
        "null"
    }

    fn version(&self) -> String {
        VERSION.to_owned()
    }

    fn prepare(&mut self, setup: &Setup) -> Result<(), DriverError> {
        self.directory = Some(setup.directory.clone());
        Ok(())
    }

    fn load(&mut self, load: &Load) -> Result<(), DriverError> {
        self.tables.push(load.table.clone());
        Ok(())
    }

    fn run(&mut self, query: &Query) -> Result<Answer, DriverError> {
        self.queries.push(query.id.clone());
        // No rows, because no work was done. A driver that returned a plausible looking answer here
        // would let a harness bug pass the correctness check on the one driver that cannot
        // possibly be correct.
        Ok(Rows::new().finish(query.ordered))
    }
}

#[cfg(test)]
mod tests {
    use bench_driver::{Format, Session};

    use super::*;

    fn setup() -> Setup {
        Setup {
            directory: PathBuf::from("scratch"),
            threads: 1,
            memory: 1 << 30,
        }
    }

    #[test]
    fn it_reports_all_three_phases() {
        let mut driver = Null::new();
        let mut session = Session::new(&mut driver);

        session.prepare(&setup()).unwrap();
        session
            .load(&Load {
                table: "hits".to_owned(),
                files: vec![PathBuf::from("hits.parquet")],
                format: Format::Parquet,
            })
            .unwrap();
        let (answer, _) = session
            .query(&Query {
                id: "q0".to_owned(),
                sql: "SELECT 1".to_owned(),
                ordered: false,
            })
            .unwrap();

        let phases = session.phases();
        assert!(phases.prepare > 0.0);
        assert!(phases.load > 0.0);
        assert!(phases.run > 0.0);
        assert_eq!(phases.tables, 1);
        assert_eq!(phases.queries, 1);

        // Nothing was computed, so there is nothing to report having computed.
        assert_eq!(answer.rows, 0);
        assert!(answer.body.is_empty());
    }

    #[test]
    fn it_records_what_it_was_asked_to_do() {
        let mut driver = Null::new();
        {
            let mut session = Session::new(&mut driver);
            session.prepare(&setup()).unwrap();
            session
                .load(&Load {
                    table: "lineitem".to_owned(),
                    files: Vec::new(),
                    format: Format::Separated { separator: '|' },
                })
                .unwrap();
        }

        assert_eq!(driver.directory, Some(PathBuf::from("scratch")));
        assert_eq!(driver.tables, ["lineitem"]);
        assert!(driver.queries.is_empty());
    }

    #[test]
    fn it_names_itself_without_a_version_in_the_name() {
        let driver = Null::new();
        assert_eq!(driver.name(), "null");
        assert!(!driver.name().contains(|c: char| c.is_ascii_digit()));
        assert_eq!(driver.version(), VERSION);
    }
}
