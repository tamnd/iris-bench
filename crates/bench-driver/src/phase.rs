//! The three phases, and the only way to get a driver to do any of them.
//!
//! Conflating preparation, loading and querying is the most common way a benchmark accidentally
//! measures the wrong thing. Load time charged to query time makes a system look slow. Index build
//! time hidden inside preparation makes it look fast. Neither is usually deliberate, and both are
//! easy to do by accident when each driver decides for itself where one phase ends and the next
//! begins.
//!
//! So the driver does not decide, and the driver does not report its own timings either. A
//! [`Session`] borrows the driver exclusively, calls the three methods itself, and times each call
//! from the outside. While a session exists nothing else can reach the driver, so there is no
//! second path by which work could happen untimed. A driver that wants to look fast has to actually
//! be fast, because the clock is not in its hands.
//!
//! # What the ordering rule is for
//!
//! A session refuses to load before preparing and refuses to query before loading. The second of
//! those is the one that earns its keep: a system that answered a query without ever being loaded
//! read the data during preparation, and the whole point of the split is that this shows up rather
//! than being absorbed into a phase nobody looks at.
//!
//! Preparation happens once. Loading can happen many times, because TPC-H is eight tables and
//! `ClickBench` is one, and a driver should not have to pretend otherwise.

use std::fmt;

use crate::{Answer, Driver, DriverError, Load, Query, Setup};

/// Which of the three phases.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Starting the system and applying its configuration. No data is in sight yet.
    Prepare,
    /// Getting a table in. Whether that means ingesting it or registering a file where it lies is
    /// the difference this phase exists to make visible.
    Load,
    /// Answering a query.
    Run,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Prepare => "prepare",
            Self::Load => "load",
            Self::Run => "run",
        })
    }
}

/// What each phase cost, in nanoseconds.
///
/// Every driver reports all three, always. A driver that reads its files in place rather than
/// ingesting them has a small load and a large run, one that builds a copy has the opposite, and
/// both of those are true things about the system that a single total would hide.
#[derive(Clone, Copy, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Phases {
    /// Nanoseconds spent in [`Driver::prepare`].
    pub prepare: f64,
    /// Nanoseconds spent in [`Driver::load`], summed over every table.
    pub load: f64,
    /// Nanoseconds spent in [`Driver::run`], summed over every query.
    pub run: f64,
    /// How many tables were loaded.
    pub tables: u32,
    /// How many queries were answered.
    pub queries: u32,
}

impl Phases {
    /// Everything the driver was in, in nanoseconds.
    ///
    /// Reported alongside the three rather than instead of them. A total on its own is the number
    /// this module exists to stop being the only one published.
    #[must_use]
    pub fn total(&self) -> f64 {
        self.prepare + self.load + self.run
    }
}

/// One driver, from preparation to the last query, with a clock on every phase.
///
/// Constructed with [`Session::new`], which takes the driver mutably for as long as the session
/// lives.
pub struct Session<'a> {
    /// The driver being measured. Held mutably so that nothing else can call it while the session
    /// is open, which is what makes "every phase is timed" a property rather than a convention.
    driver: &'a mut dyn Driver,
    /// What has been spent so far.
    phases: Phases,
    /// Whether preparation has happened.
    prepared: bool,
    /// Whether at least one table has been loaded.
    loaded: bool,
}

impl fmt::Debug for Session<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("driver", &self.driver.name())
            .field("phases", &self.phases)
            .field("prepared", &self.prepared)
            .field("loaded", &self.loaded)
            .finish()
    }
}

impl<'a> Session<'a> {
    /// Takes the driver for the duration of a run.
    pub fn new(driver: &'a mut dyn Driver) -> Self {
        Self {
            driver,
            phases: Phases::default(),
            prepared: false,
            loaded: false,
        }
    }

    /// Starts the system and applies its configuration, and returns what that cost in nanoseconds.
    ///
    /// # Errors
    ///
    /// If the driver fails, or if preparation has already happened.
    pub fn prepare(&mut self, setup: &Setup) -> Result<f64, DriverError> {
        if self.prepared {
            return Err(DriverError::Repeated {
                phase: Phase::Prepare,
            });
        }
        let (result, elapsed) = bench_core::time(|| self.driver.prepare(setup));
        result?;
        self.prepared = true;
        self.phases.prepare += elapsed;
        Ok(elapsed)
    }

    /// Gets one table in, and returns what that cost in nanoseconds.
    ///
    /// # Errors
    ///
    /// If the driver fails, or if the system has not been prepared.
    pub fn load(&mut self, load: &Load) -> Result<f64, DriverError> {
        if !self.prepared {
            return Err(DriverError::OutOfOrder {
                phase: Phase::Load,
                wanted: Phase::Prepare,
            });
        }
        let (result, elapsed) = bench_core::time(|| self.driver.load(load));
        result?;
        self.loaded = true;
        self.phases.load += elapsed;
        self.phases.tables += 1;
        Ok(elapsed)
    }

    /// Answers one query, and returns the answer along with what it cost in nanoseconds.
    ///
    /// # Errors
    ///
    /// If the driver fails, or if nothing has been loaded.
    pub fn query(&mut self, query: &Query) -> Result<(Answer, f64), DriverError> {
        if !self.loaded {
            return Err(DriverError::OutOfOrder {
                phase: Phase::Run,
                wanted: Phase::Load,
            });
        }
        let (result, elapsed) = bench_core::time(|| self.driver.run(query));
        let answer = result?;
        self.phases.run += elapsed;
        self.phases.queries += 1;
        Ok((answer, elapsed))
    }

    /// What has been spent so far, by phase.
    #[must_use]
    pub fn phases(&self) -> Phases {
        self.phases
    }

    /// What the driver calls itself, for the result row.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.driver.name()
    }

    /// What version of the system this is, for the result row.
    #[must_use]
    pub fn version(&self) -> String {
        self.driver.version()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{Format, Rows, Value};

    /// A driver that records what it was asked to do and takes no time doing it.
    #[derive(Default)]
    struct Recording {
        calls: Vec<Phase>,
        fails: Option<Phase>,
    }

    impl Driver for Recording {
        fn name(&self) -> &'static str {
            "recording"
        }

        fn version(&self) -> String {
            "0".to_owned()
        }

        fn prepare(&mut self, _setup: &Setup) -> Result<(), DriverError> {
            self.calls.push(Phase::Prepare);
            self.fail(Phase::Prepare)
        }

        fn load(&mut self, _load: &Load) -> Result<(), DriverError> {
            self.calls.push(Phase::Load);
            self.fail(Phase::Load)?;
            Ok(())
        }

        fn run(&mut self, _query: &Query) -> Result<Answer, DriverError> {
            self.calls.push(Phase::Run);
            self.fail(Phase::Run)?;
            let mut rows = Rows::new();
            rows.push([Value::Int(1)])?;
            Ok(rows.finish(false))
        }
    }

    impl Recording {
        fn fail(&self, phase: Phase) -> Result<(), DriverError> {
            if self.fails == Some(phase) {
                return Err(DriverError::unsupported(phase.to_string(), "asked to fail"));
            }
            Ok(())
        }
    }

    fn setup() -> Setup {
        Setup {
            directory: PathBuf::from("scratch"),
            threads: 1,
            memory: 1 << 30,
        }
    }

    fn load() -> Load {
        Load {
            table: "hits".to_owned(),
            files: vec![PathBuf::from("hits.parquet")],
            format: Format::Parquet,
            projection: None,
        }
    }

    fn query() -> Query {
        Query {
            id: "q0".to_owned(),
            sql: "SELECT 1".to_owned(),
            ordered: false,
        }
    }

    #[test]
    fn every_phase_is_timed_and_counted() {
        let mut driver = Recording::default();
        let mut session = Session::new(&mut driver);

        session.prepare(&setup()).unwrap();
        session.load(&load()).unwrap();
        session.load(&load()).unwrap();
        session.query(&query()).unwrap();
        session.query(&query()).unwrap();

        let phases = session.phases();
        assert_eq!(phases.tables, 2);
        assert_eq!(phases.queries, 2);
        assert!(phases.prepare > 0.0);
        assert!(phases.load > 0.0);
        assert!(phases.run > 0.0);
        assert!(phases.total() >= phases.prepare + phases.load + phases.run - 1.0);
    }

    #[test]
    fn a_query_before_a_load_is_refused_rather_than_answered() {
        // A system that can answer without having been loaded read the data during preparation,
        // which is the exact thing the split exists to make visible.
        let mut driver = Recording::default();
        let error = {
            let mut session = Session::new(&mut driver);
            session.prepare(&setup()).unwrap();
            session.query(&query()).unwrap_err()
        };

        assert!(matches!(
            error,
            DriverError::OutOfOrder {
                phase: Phase::Run,
                wanted: Phase::Load
            }
        ));
        assert_eq!(driver.calls, [Phase::Prepare]);
    }

    #[test]
    fn a_load_before_a_prepare_is_refused_rather_than_run() {
        let mut driver = Recording::default();
        let error = Session::new(&mut driver).load(&load()).unwrap_err();

        assert!(matches!(
            error,
            DriverError::OutOfOrder {
                phase: Phase::Load,
                wanted: Phase::Prepare
            }
        ));
        assert!(driver.calls.is_empty());
    }

    #[test]
    fn preparing_twice_is_refused() {
        let mut driver = Recording::default();
        let mut session = Session::new(&mut driver);

        session.prepare(&setup()).unwrap();
        let error = session.prepare(&setup()).unwrap_err();
        assert!(matches!(
            error,
            DriverError::Repeated {
                phase: Phase::Prepare
            }
        ));
    }

    #[test]
    fn a_phase_that_failed_is_not_counted_as_one_that_happened() {
        let mut driver = Recording {
            fails: Some(Phase::Load),
            ..Recording::default()
        };
        let mut session = Session::new(&mut driver);

        session.prepare(&setup()).unwrap();
        session.load(&load()).unwrap_err();

        assert_eq!(session.phases().tables, 0);
        // And the run phase is still closed, because nothing was ever loaded.
        assert!(session.query(&query()).is_err());
    }

    #[test]
    fn the_phases_read_as_the_words_a_person_would_use() {
        assert_eq!(Phase::Prepare.to_string(), "prepare");
        assert_eq!(Phase::Load.to_string(), "load");
        assert_eq!(Phase::Run.to_string(), "run");
    }
}
