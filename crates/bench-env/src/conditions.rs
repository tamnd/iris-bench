//! What a machine is doing at the moment the capture is taken.
//!
//! Kept apart from [`crate::Facts`] because none of it is hashed. Free memory and load average move
//! between one measurement and the next, so a hash over them would be unique per row and could not
//! group anything, which is the one job the environment hash has.
//!
//! What protects a measurement from a busy machine is not the hash. It is that these readings feed
//! gates, and a gate that fails stops the run.

use serde::{Deserialize, Serialize};

/// How much CPU a process has to be using before it counts as competition.
///
/// Five percent of one processor. Below that is the background of any desktop or server: a handful
/// of daemons waking up on a timer, none of which will evict a working set. Above it is something
/// doing work, and something doing work while a measurement is taken is exactly what a gate is for.
const BUSY_PERCENT: f32 = 5.0;

/// What the machine is doing right now.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub struct Conditions {
    /// Memory available without swapping, in bytes.
    pub available_memory_bytes: u64,
    /// The one minute load average, where the platform keeps one.
    ///
    /// Windows does not, and this is `None` there rather than zero, because zero is a reading and
    /// what is true is that there is nothing to read.
    pub load_average: Option<f64>,
    /// How many other processes are using a noticeable share of a processor.
    pub busy_processes: usize,
}

impl Conditions {
    /// Observes the machine, which takes a moment on purpose.
    ///
    /// Per process CPU usage is a rate, so it needs two readings with a gap between them. The gap is
    /// the shortest the platform will give a meaningful answer over. A capture that skipped it would
    /// report every process at zero and the busy process gate would pass on a machine compiling
    /// something.
    #[must_use]
    pub fn observe() -> Self {
        let mut system = sysinfo::System::new_all();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        system.refresh_all();

        let ours = sysinfo::get_current_pid().ok();
        let busy_processes = system
            .processes()
            .iter()
            .filter(|(pid, process)| Some(**pid) != ours && process.cpu_usage() >= BUSY_PERCENT)
            .count();

        Self {
            available_memory_bytes: system.available_memory(),
            load_average: load_average(),
            busy_processes,
        }
    }
}

/// The one minute load average, or nothing on a platform that does not have the idea.
fn load_average() -> Option<f64> {
    if cfg!(target_os = "windows") {
        return None;
    }
    Some(sysinfo::System::load_average().one)
}
