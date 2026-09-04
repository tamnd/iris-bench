//! The trait every system under test implements.
//!
//! The prepare, load and run split is fixed here for everyone, so that no
//! system can move work into an untimed phase that another system pays for.
//!
//! Nothing is implemented yet. See the milestone that owns this crate in
//! `docs/ROADMAP.md`.

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
