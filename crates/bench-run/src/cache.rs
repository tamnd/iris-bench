//! Dropping what the operating system is holding on to.
//!
//! `ClickBench` reports a cold number and a hot number, and the cold one only means anything if the
//! page cache was really empty when the query started. That is a privileged operation on a machine,
//! which is why it lives in the runner rather than in the crate that describes the protocol.
//!
//! Nothing here ever claims a drop it did not do. `bench_workload::Cold::Kept` carries the reason
//! and the reason travels all the way into the result, because a warm first run published under the
//! word cold is the single most misleading row a benchmark can print.

use bench_workload::{Cold, PageCache};

/// Where Linux takes the request.
#[cfg(target_os = "linux")]
const DROP_CACHES: &str = "/proc/sys/vm/drop_caches";

/// Asks the kernel to drop its page cache.
///
/// On Linux this syncs and then writes to `/proc/sys/vm/drop_caches`, which needs root. A process
/// without it gets [`Cold::Kept`] with the error the kernel gave, so the run still happens and the
/// numbers still get recorded, labelled as what they are.
///
/// Anywhere else this reports [`Cold::Kept`] without trying. There are things that do something
/// similar on other systems, and every one of them would be a second mechanism with its own
/// semantics for this repository to trust. The machines that publish numbers here are Linux, and
/// one mechanism that is understood is worth more than three that are approximately right.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DropCaches;

impl PageCache for DropCaches {
    #[cfg(target_os = "linux")]
    fn evict(&mut self) -> Cold {
        // Dirty pages cannot be freed, so a drop without a sync in front of it drops whatever
        // happened to be clean and leaves the rest. That is worse than not dropping at all, because
        // it looks like it worked.
        //
        // SAFETY: sync takes no arguments, returns nothing, and has no failure mode to check. It
        // is unsafe here only because it is a foreign function.
        unsafe { libc::sync() };
        match std::fs::write(DROP_CACHES, "3") {
            Ok(()) => Cold::Dropped,
            Err(error) => Cold::Kept {
                why: format!("writing to {DROP_CACHES} needs root: {error}"),
            },
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn evict(&mut self) -> Cold {
        Cold::Kept {
            why: format!(
                "there is no page cache drop this repository trusts on {}",
                std::env::consts::OS
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether this process could drop the cache even if the mechanism existed.
    ///
    /// `cfg!` below is a value and not a compilation switch, so the call still has to compile
    /// everywhere the tests run even where the branch is never taken. Windows has no effective user
    /// id to ask about, and the answer it needs is the same one every non root process gets.
    #[cfg(unix)]
    fn privileged() -> bool {
        // SAFETY: geteuid takes no arguments and cannot fail.
        unsafe { libc::geteuid() == 0 }
    }

    /// Whether this process could drop the cache even if the mechanism existed.
    #[cfg(not(unix))]
    fn privileged() -> bool {
        false
    }

    #[test]
    fn a_process_that_cannot_drop_the_cache_says_why_rather_than_claiming_it_did() {
        // The test the whole module exists for. Almost every machine running this is not root, so
        // almost every run of it goes down the second branch, which is the branch that matters.
        let cold = DropCaches.evict();
        if cfg!(target_os = "linux") && privileged() {
            assert!(cold.is_cold());
        } else {
            let Cold::Kept { why } = cold else {
                panic!("nothing without the privilege may report a cold cache");
            };
            assert!(!why.is_empty(), "a kept cache says what kept it");
        }
    }
}
