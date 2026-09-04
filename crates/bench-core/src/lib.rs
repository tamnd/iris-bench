//! Timing, repetition and the result row.
//!
//! No benchmark lives in this crate. It holds the measurement loop, the
//! bootstrap confidence interval on the median, and the row schema that every
//! result is written as. Keeping benchmarks out of it is what lets the timing
//! code be reviewed in one sitting.
//!
//! # How a measurement is taken
//!
//! A pilot runs a small balanced design across builds, processes and iterations. [`Pilot::decompose`]
//! splits the variance across those three levels and [`plan`] spends the repetition budget at the
//! level that carries it. Then [`Timing::run`] collects samples until the confidence interval on
//! the median is narrow enough, and [`summarise`] produces the row.
//!
//! ```
//! use bench_core::{Bootstrap, Stop, Timing};
//!
//! let timing = Timing {
//!     warmup: 2,
//!     stop: Stop::After { samples: 32 },
//!     check_every: 1,
//!     bootstrap: Bootstrap::default(),
//! };
//!
//! // A workload that always takes the same time, so the example has a predictable answer.
//! let series = timing.run(|| 1_000.0);
//! let summary = series.summary().expect("32 samples is not nothing");
//!
//! assert_eq!(summary.n, 32);
//! assert!((summary.median - 1_000.0).abs() < f64::EPSILON);
//! ```
//!
//! # The one rule worth reading the source for
//!
//! [`Stop::should_stop`] is handed the series being measured and nothing else. It cannot see a
//! baseline or a rival, because adding repetitions until a comparison turns significant is how a
//! result gets manufactured, and the defence against that is not having the comparison in scope at
//! the point where the decision is made.

mod measure;
mod repetition;
mod stats;

pub use measure::{Series, Stop, Timing, time};
pub use repetition::{Components, Level, Pilot, Plan, plan};
pub use stats::{Bootstrap, Summary, summarise};

/// Bumped whenever the way a measurement is taken changes.
///
/// Change point detection resets at a boundary where this changes rather than
/// running across it, because otherwise a change of method shows up as a
/// regression in whichever system happened to be measured next.
pub const METHODOLOGY_VERSION: u32 = 1;

/// What can go wrong before a measurement is even summarised.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A pilot ran too few repetitions at some level to say anything about it.
    #[error(
        "a pilot needs at least two repetitions at every level, got {builds} builds, {processes} processes and {iterations} iterations"
    )]
    PilotTooSmall {
        /// How many builds the pilot ran.
        builds: usize,
        /// How many processes per build.
        processes: usize,
        /// How many iterations per process.
        iterations: usize,
    },
    /// A pilot design was ragged.
    #[error(
        "a pilot design has to be balanced, so that the variance decomposition is arithmetic a reader can check"
    )]
    Unbalanced,
    /// A sample was not a finite number.
    #[error("a sample was not a finite number, which means the clock or the harness is broken")]
    NotFinite,
}
