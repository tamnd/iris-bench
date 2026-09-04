//! Environment capture and eligibility gates.
//!
//! A machine that fails a gate does not produce a slower number, it produces a
//! number that should not be published. So the gates abort the run and name the
//! gate that failed, and that behaviour is not configurable.
//!
//! Nothing is implemented yet. See the milestone that owns this crate in
//! `docs/ROADMAP.md`.

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
