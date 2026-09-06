//! The runner.
//!
//! Owns process isolation, randomised ordering with a recorded seed, the five storage tiers, and
//! cache control. Warm up is the runner's job and not the driver's, which is how one system does
//! not get warmed while another does not.
//!
//! Cache control is the part that exists so far. [`DropCaches`] is what
//! `bench_workload::protocol` asks before each query's three runs, and it is here rather than there
//! because dropping the page cache is a privileged operation on a machine and `bench-workload` is a
//! description of a protocol. The rest is still to come. See the milestone that owns this crate in
//! `docs/ROADMAP.md`.

pub mod cache;

pub use cache::DropCaches;

/// The version of this crate, as reported by build metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
