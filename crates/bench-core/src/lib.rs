//! Timing, repetition and the result row.
//!
//! No benchmark lives in this crate. It holds the measurement loop, the
//! bootstrap confidence interval on the median, and the row schema that every
//! result is written as. Keeping benchmarks out of it is what lets the timing
//! code be reviewed in one sitting.
//!
//! Nothing is implemented yet. See the milestone that owns this crate in
//! `docs/ROADMAP.md`.

/// Bumped whenever the way a measurement is taken changes.
///
/// Change point detection resets at a boundary where this changes rather than
/// running across it, because otherwise a change of method shows up as a
/// regression in whichever system happened to be measured next.
pub const METHODOLOGY_VERSION: u32 = 1;
