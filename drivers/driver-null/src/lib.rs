//! A driver that does nothing.
//!
//! Running the harness against a system that does no work measures what the
//! harness itself costs. The instrumentation budget is one percent, and this is
//! how it is checked.
//!
//! Nothing is implemented yet. See the milestone that owns this crate in
//! `docs/ROADMAP.md`.

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
