//! Rendering.
//!
//! The rules that stop a misleading table being drawn are code in this crate
//! and not editorial policy. A comparison whose interval crosses one renders as
//! no measurable difference. A geometric mean never appears without its per
//! query table on the same page. Failed queries are shown rather than dropped.
//!
//! What is implemented so far is the wording rules for workloads derived from a
//! specification somebody else owns, which `docs/LICENSING.md` states in prose
//! and [`tpc`] states as code. Those came first because they are the rules where
//! getting it wrong is not a rendering bug. The rest is the milestone that owns
//! this crate in `docs/ROADMAP.md`.

pub mod page;
pub mod tpc;

pub use page::{Page, Row};
pub use tpc::{Cited, Family, WordingError};

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
