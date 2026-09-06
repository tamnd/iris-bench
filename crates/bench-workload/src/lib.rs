//! The published workloads, carried verbatim, and the rules their authors run them under.
//!
//! A benchmark result is only comparable with somebody else's if it asked the same questions in the
//! same way. This crate holds both halves of that: the query text exactly as it is published, with
//! the address it came from and the digest it hashed to, and the protocol the publishers run it
//! under. Neither half is invented here, and the tests in this crate exist mostly to prove that.
//!
//! ```
//! use bench_workload::clickbench::{self, Dialect};
//!
//! let workload = clickbench::workload(Dialect::DuckDb);
//!
//! assert_eq!(workload.name, "clickbench");
//! assert_eq!(workload.table, "hits");
//! assert_eq!(workload.queries.len(), clickbench::QUERIES);
//! assert_eq!(workload.queries[0].id, "q0");
//!
//! // And the results say where the text came from, so a reader does not have to take this
//! // repository's word for what ClickBench is.
//! assert!(workload.source.url.contains("ClickBench"));
//! ```
//!
//! # The three things this crate refuses to do
//!
//! **Rewrite a query.** `ClickBench` publishes a different `queries.sql` per system because the
//! systems speak different dialects, so each driver gets the file published for it. A rewrite that
//! upstream has not published is a deviation and belongs in that driver's `CONFIG.md`, not in a
//! string here.
//!
//! **Time anything.** `bench_driver::Session` holds the clock, and [`protocol::measure`] asks it
//! for the numbers rather than reading one of its own.
//!
//! **Call a warm run cold.** Dropping the page cache is privileged and does not always work.
//! [`protocol::Cold`] carries whether it did, all the way into the result.

pub mod agree;
pub mod clickbench;
pub mod digest;
pub mod protocol;
pub mod workload;

pub use agree::{Comparison, Disagreement, Reading, Unstable, compare};
pub use clickbench::Dialect;
pub use digest::{Digest, ParseDigestError};
pub use protocol::{Cold, Measured, Outcome, PageCache, RUNS, Report, Run, Warm, measure};
pub use workload::{Source, Workload};

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
