//! The trait every system under test implements.
//!
//! The prepare, load and run split is fixed here for everyone, so that no system can move work into
//! an untimed phase that another system pays for. A driver implements [`Driver`], and a [`Session`]
//! drives it and holds the clock.
//!
//! ```
//! use bench_driver::{Answer, Driver, DriverError, Format, Load, Query, Rows, Session, Setup, Value};
//!
//! struct Counting {
//!     rows: i64,
//! }
//!
//! impl Driver for Counting {
//!     fn name(&self) -> &'static str {
//!         "counting"
//!     }
//!
//!     fn version(&self) -> String {
//!         "1".to_owned()
//!     }
//!
//!     fn prepare(&mut self, _setup: &Setup) -> Result<(), DriverError> {
//!         Ok(())
//!     }
//!
//!     fn load(&mut self, load: &Load) -> Result<(), DriverError> {
//!         self.rows += load.files.len() as i64;
//!         Ok(())
//!     }
//!
//!     fn run(&mut self, _query: &Query) -> Result<Answer, DriverError> {
//!         let mut rows = Rows::new();
//!         rows.push([Value::Int(self.rows)])?;
//!         Ok(rows.finish(false))
//!     }
//! }
//!
//! let mut driver = Counting { rows: 0 };
//! let mut session = Session::new(&mut driver);
//!
//! session.prepare(&Setup {
//!     directory: std::path::PathBuf::from("scratch"),
//!     threads: 1,
//!     memory: 1 << 30,
//! })?;
//! session.load(&Load {
//!     table: "hits".to_owned(),
//!     files: vec![std::path::PathBuf::from("hits.parquet")],
//!     format: Format::Parquet,
//!     projection: None,
//! })?;
//! let (answer, _nanoseconds) = session.query(&Query {
//!     id: "q0".to_owned(),
//!     sql: "SELECT 1".to_owned(),
//!     ordered: false,
//! })?;
//!
//! assert_eq!(answer.body, "1\n");
//!
//! // All three, always, whatever the system did in each.
//! let phases = session.phases();
//! assert!(phases.prepare > 0.0 && phases.load > 0.0 && phases.run > 0.0);
//! # Ok::<(), DriverError>(())
//! ```
//!
//! # The two things a driver does not get to decide
//!
//! **Where one phase ends.** [`Setup`] carries no file paths, so a load cannot hide inside
//! preparation and no index can be built there. [`Session`] refuses to answer a query before
//! something has been loaded, so a system that read the data early has to say so.
//!
//! **What its results look like.** [`Answer`] is a canonical rendering built through [`Rows`], and
//! [`crate::answer`] documents the two decisions in it that have a real cost. Drivers do not hash
//! their own output, because a driver that hashes its own output can agree with itself about a
//! wrong answer.

pub mod answer;
mod driver;
mod phase;

pub use answer::{Answer, Rows, Value, WidthError};
pub use driver::{Driver, DriverError, Format, Load, Query, Setup};
pub use phase::{Phase, Phases, Session};

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
