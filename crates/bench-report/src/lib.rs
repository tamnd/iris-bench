//! Rendering.
//!
//! The rules that stop a misleading table being drawn are code in this crate
//! and not editorial policy. A comparison whose interval crosses one renders as
//! no measurable difference. A geometric mean never appears without its per
//! query table on the same page. Failed queries are shown rather than dropped.
//!
//! Nothing is implemented yet. See the milestone that owns this crate in
//! `docs/ROADMAP.md`.

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
