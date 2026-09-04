//! The runner.
//!
//! Owns process isolation, randomised ordering with a recorded seed, the five
//! storage tiers, and cache control. Warm up is the runner's job and not the
//! driver's, which is how one system does not get warmed while another does
//! not.
//!
//! Nothing is implemented yet. See the milestone that owns this crate in
//! `docs/ROADMAP.md`.

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
