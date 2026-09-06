//! Append only result storage and the claim ledger.
//!
//! Raw iterations are stored, never summaries. Aggregation happens at read
//! time, because storing only medians would make it impossible to re-analyse
//! with a different statistic later and re-running is expensive.
//!
//! Nothing is deleted. Corrections are new rows that reference the run they
//! correct.
//!
//! What is implemented so far is [`claim`], which is the half of this crate
//! that decides what a result means rather than where it is kept. It carries
//! the registered claims, the five verdicts and the arithmetic that turns a
//! reading into one of them. The storage half is the milestone that owns this
//! crate in `docs/ROADMAP.md`.

pub mod claim;

pub use claim::{Attempt, Bar, Caveat, Claim, Measured, Purpose, Refused, Ungradable, Verdict};

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
