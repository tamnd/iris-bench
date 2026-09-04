//! Append only result storage and the claim ledger.
//!
//! Raw iterations are stored, never summaries. Aggregation happens at read
//! time, because storing only medians would make it impossible to re-analyse
//! with a different statistic later and re-running is expensive.
//!
//! Nothing is deleted. Corrections are new rows that reference the run they
//! correct.
//!
//! Nothing is implemented yet. See the milestone that owns this crate in
//! `docs/ROADMAP.md`.

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
