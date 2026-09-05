//! Environment capture and machine eligibility gates.
//!
//! Before a run takes a single measurement it asks this crate whether the machine is fit to produce
//! the kind of number the run wants. The answer is yes, or it is a refusal naming the gate that
//! failed. There is no third answer and there is no override, because a number with a warning
//! attached is a number that gets copied without the warning and then it is on a slide somewhere.
//!
//! ```no_run
//! use bench_env::{Capture, Permit};
//!
//! let capture = Capture::take();
//! capture.require(Permit::Durations)?;
//! # Ok::<(), bench_env::Refusal>(())
//! ```
//!
//! # The two halves of a capture
//!
//! [`Facts`] is what the machine is: the processor, the memory fitted, the governor, the boost
//! state, whether a run was pinned, whether there is a hypervisor underneath. It is stable until
//! somebody reconfigures the machine, and it is what gets hashed into every result row.
//!
//! [`Conditions`] is what the machine is doing: free memory, load average, how many other processes
//! are busy. It moves between one measurement and the next, so it is recorded and gated and it is
//! not hashed. Hashing it would give every row a hash of its own, and grouping rows is the only
//! thing the hash is for.
//!
//! # Where the strictness actually is
//!
//! Not in the gates. A gate can only fail on something it can read, and the settings that matter
//! most are the ones some platforms do not expose at all. So the rule that does the work is in
//! [`policy::ceiling`]: a machine whose clock settings cannot be read produces ratios rather than
//! durations, on the grounds that being unable to see a setting is not evidence that the setting is
//! right. That is why the workstation running Windows is capped below the same workstation running
//! Linux, and why the cap shows up as a ceiling with a reason rather than as a gate that quietly
//! passed.

mod class;
mod conditions;
mod facts;
mod gate;
pub mod policy;

pub use class::{Class, classify};
pub use conditions::Conditions;
pub use facts::{Facts, Hypervisor, Setting};
pub use gate::{Ceiling, Gate, Outcome, Permit, Refusal};

use bench_core::{EnvironmentHash, METHODOLOGY_VERSION};
use serde::{Deserialize, Serialize};

/// The version of this crate, recorded so a capture says which code produced it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Everything known about the machine a measurement was taken on.
///
/// This is what gets written to `environment.json` beside a set of results, and [`Capture::hash`] is
/// what goes into every row of them.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub struct Capture {
    /// The methodology version the measurement was taken under.
    pub methodology: u32,
    /// Which hardware class this machine was identified as.
    pub class: Class,
    /// The most this machine can produce, and why.
    pub ceiling: Ceiling,
    /// What the machine is.
    pub facts: Facts,
    /// What the machine was doing when the capture was taken.
    pub conditions: Conditions,
    /// Every gate that could be evaluated here, and what it said.
    pub gates: Vec<Gate>,
    /// The digest of [`Capture::facts`], which every result row carries.
    pub hash: EnvironmentHash,
}

impl Capture {
    /// Reads the machine and evaluates its gate set.
    ///
    /// Takes a moment, because per process CPU usage is a rate and a rate needs two readings with a
    /// gap between them.
    #[must_use]
    pub fn take() -> Self {
        Self::of(Facts::read(), Conditions::observe())
    }

    /// Classifies and gates a machine that has already been read.
    ///
    /// Private because a capture is supposed to describe a machine somebody measured on, and a
    /// constructor that takes the facts as an argument is a constructor for a machine that does not
    /// exist. The tests want exactly that, which is the one legitimate use.
    fn of(facts: Facts, conditions: Conditions) -> Self {
        let class = classify(&facts);

        Self {
            methodology: METHODOLOGY_VERSION,
            hash: hash(class, &facts),
            ceiling: policy::ceiling(class, &facts),
            gates: policy::gates(class, &facts, &conditions),
            class,
            facts,
            conditions,
        }
    }

    /// The first gate that failed, if one did.
    #[must_use]
    pub fn failed(&self) -> Option<&Gate> {
        self.gates.iter().find(|gate| !gate.outcome.passed())
    }

    /// What this machine may be used to produce as things stand.
    ///
    /// A failed gate takes it to nothing rather than down one step. A machine with something else
    /// running on it is not a machine whose ratios are trustworthy either, because the interference
    /// does not have to land evenly on the two sides of a ratio.
    #[must_use]
    pub fn permits(&self) -> Permit {
        if self.failed().is_some() {
            Permit::Nothing
        } else {
            self.ceiling.permit
        }
    }

    /// Says whether the run may go ahead, and refuses by name when it may not.
    ///
    /// # Errors
    ///
    /// [`Refusal::Gate`] when a gate failed, naming it and saying what would have passed.
    /// [`Refusal::Ceiling`] when every gate passed and this machine still does not produce what was
    /// asked for.
    pub fn require(&self, asked: Permit) -> Result<(), Refusal> {
        // Gates first. A failing gate is something a person can go and change, and a ceiling is
        // usually not, so reporting the ceiling on a machine that also has something running on it
        // would send them off to argue with the machine notes about the wrong problem.
        if let Some(gate) = self.failed() {
            let Outcome::Fail { observed, wanted } = &gate.outcome else {
                unreachable!("failed() only returns a gate whose outcome is a failure")
            };
            return Err(Refusal::Gate {
                gate: gate.name.clone(),
                observed: observed.clone(),
                wanted: wanted.clone(),
                asked,
            });
        }

        if self.ceiling.permit < asked {
            return Err(Refusal::Ceiling {
                class: self.class,
                permits: self.ceiling.permit,
                because: self.ceiling.because.clone(),
                asked,
            });
        }

        Ok(())
    }

    /// The capture as the JSON that gets written beside a set of results.
    ///
    /// # Panics
    ///
    /// If the capture cannot be serialised, which would mean a field was added that serde cannot
    /// represent. There is no such field and there is no recovery from one.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a capture is made only of serialisable fields")
    }
}

/// Hashes what the machine is, and nothing about what it was doing.
///
/// The class is in the hash as well as the facts it was derived from. It is redundant today, and it
/// stops a change to the classification rules from silently producing the same hash for a machine
/// that is now understood differently.
fn hash(class: Class, facts: &Facts) -> EnvironmentHash {
    #[derive(Serialize)]
    struct Hashed<'a> {
        methodology: u32,
        class: Class,
        facts: &'a Facts,
    }

    let canonical = serde_json::to_vec(&Hashed {
        methodology: METHODOLOGY_VERSION,
        class,
        facts,
    })
    .expect("the facts are made only of serialisable fields");

    EnvironmentHash::from_bytes(*blake3::hash(&canonical).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::{Capture, Ceiling, Class, Conditions, Facts, Hypervisor, Permit, Refusal, Setting};

    /// The workstation, configured the way a timing run wants it.
    fn workstation() -> Facts {
        Facts {
            arch: "x86_64".to_owned(),
            os: "linux".to_owned(),
            os_version: Setting::Reading("24.04".to_owned()),
            kernel: Setting::Reading("6.8.0-51-generic".to_owned()),
            cpu: "13th Gen Intel(R) Core(TM) i9-13900K".to_owned(),
            logical_cpus: 32,
            physical_cores: Some(24),
            memory_bytes: 64 * 1024 * 1024 * 1024,
            hypervisor: Hypervisor::None,
            governor: Setting::Reading("performance".to_owned()),
            turbo: Setting::Reading("off".to_owned()),
            affinity: Setting::Reading("0-15".to_owned()),
        }
    }

    /// One of the shared tenancy servers.
    fn epyc_guest() -> Facts {
        Facts {
            cpu: "AMD EPYC 7002 series".to_owned(),
            logical_cpus: 8,
            physical_cores: Some(8),
            memory_bytes: 24 * 1024 * 1024 * 1024,
            hypervisor: Hypervisor::Guest("QEMU".to_owned()),
            governor: Setting::Unreadable("the guest exposes no cpufreq governor".to_owned()),
            turbo: Setting::Unreadable("the guest exposes no boost switch".to_owned()),
            affinity: Setting::Reading("0-3".to_owned()),
            ..workstation()
        }
    }

    /// A machine with nothing else happening on it.
    fn idle(facts: &Facts) -> Conditions {
        Conditions {
            available_memory_bytes: facts.memory_bytes / 2,
            load_average: Some(0.1),
            busy_processes: 0,
        }
    }

    #[test]
    fn a_gate_that_fails_names_itself_in_the_refusal() {
        let facts = workstation();
        let conditions = Conditions {
            available_memory_bytes: 1024 * 1024 * 1024,
            ..idle(&facts)
        };

        let refusal = Capture::of(facts, conditions)
            .require(Permit::Durations)
            .expect_err("a machine with one gigabyte free is not fit to measure on");

        let Refusal::Gate { gate, wanted, .. } = refusal else {
            panic!("a failing gate has to refuse as a gate, not as a ceiling: {refusal:?}")
        };
        assert_eq!(gate, "memory-headroom");
        assert!(wanted.contains("16.0 GiB"), "wanted {wanted}");
    }

    #[test]
    fn a_failing_gate_is_reported_before_the_ceiling_is() {
        // Both are wrong with this machine: it is a guest, which caps it at nothing, and it also has
        // something running on it. The gate is the half a person can go and fix, so it comes first.
        let facts = epyc_guest();
        let conditions = Conditions {
            busy_processes: 3,
            ..idle(&facts)
        };

        let refusal = Capture::of(facts, conditions)
            .require(Permit::Ratios)
            .expect_err("three busy processes is not an idle machine");

        assert!(
            matches!(refusal, Refusal::Gate { ref gate, .. } if gate == "busy-processes"),
            "{refusal:?}"
        );
    }

    #[test]
    fn a_shared_tenancy_guest_is_refused_however_idle_it_looks() {
        let facts = epyc_guest();
        let conditions = idle(&facts);
        let capture = Capture::of(facts, conditions);

        assert_eq!(capture.class, Class::A);
        assert!(capture.failed().is_none(), "{:?}", capture.gates);
        assert_eq!(capture.permits(), Permit::Nothing);

        let refusal = capture
            .require(Permit::Ratios)
            .expect_err("a guest does not produce a ratio either");
        assert!(matches!(refusal, Refusal::Ceiling { .. }), "{refusal:?}");
    }

    #[test]
    fn the_workstation_produces_durations_when_its_clock_settings_can_be_read() {
        let facts = workstation();
        let conditions = idle(&facts);
        let capture = Capture::of(facts, conditions);

        assert_eq!(capture.class, Class::B);
        assert_eq!(capture.permits(), Permit::Durations);
        capture
            .require(Permit::Durations)
            .expect("a pinned workstation with boost off and the performance governor");
    }

    #[test]
    fn the_workstation_drops_to_ratios_when_they_cannot_be_read() {
        // This is the machine under Windows. Every gate that can be evaluated passes, because the
        // ones that would have caught a drifting clock cannot be evaluated at all. Being unable to
        // see a setting is not evidence that the setting is right, so the ceiling falls.
        let facts = Facts {
            os: "windows".to_owned(),
            turbo: Setting::Unreadable("windows does not expose the boost state".to_owned()),
            affinity: Setting::Unreadable("not implemented for windows".to_owned()),
            governor: Setting::Reading("High performance".to_owned()),
            ..workstation()
        };
        let conditions = idle(&facts);
        let capture = Capture::of(facts, conditions);

        assert_eq!(capture.class, Class::B);
        assert!(capture.failed().is_none(), "{:?}", capture.gates);
        assert_eq!(capture.permits(), Permit::Ratios);

        let refusal = capture
            .require(Permit::Durations)
            .expect_err("nothing checked whether the clock was steady");
        let Refusal::Ceiling { because, .. } = refusal else {
            panic!("expected a ceiling: {refusal:?}")
        };
        assert!(because.contains("the boost state"), "because {because}");
        assert!(
            because.contains("the processor affinity"),
            "because {because}"
        );
    }

    #[test]
    fn a_run_that_is_free_to_move_across_every_processor_fails_the_pinning_gate() {
        let facts = Facts {
            affinity: Setting::Reading("0-31".to_owned()),
            ..workstation()
        };
        let conditions = idle(&facts);

        let refusal = Capture::of(facts, conditions)
            .require(Permit::Durations)
            .expect_err("an unpinned run on a hybrid part has two modes in it");

        assert!(
            matches!(refusal, Refusal::Gate { ref gate, .. } if gate == "core-pinning"),
            "{refusal:?}"
        );
    }

    #[test]
    fn the_workstation_under_a_hypervisor_is_still_the_workstation() {
        let facts = Facts {
            hypervisor: Hypervisor::Wsl,
            ..workstation()
        };
        let conditions = idle(&facts);
        let capture = Capture::of(facts, conditions);

        assert_eq!(capture.class, Class::B, "the silicon has not changed");
        assert_eq!(capture.permits(), Permit::Ratios, "what it can say has");
    }

    #[test]
    fn the_environment_hash_ignores_what_the_machine_was_doing() {
        let facts = workstation();
        let busy = Conditions {
            available_memory_bytes: facts.memory_bytes / 3,
            load_average: Some(9.0),
            busy_processes: 4,
            ..idle(&facts)
        };

        let quiet = Capture::of(facts.clone(), idle(&facts));
        let loud = Capture::of(facts, busy);

        assert_eq!(
            quiet.hash, loud.hash,
            "a hash that moved with the load average could not group anything"
        );
        assert_ne!(
            quiet.permits(),
            loud.permits(),
            "the load average is supposed to be caught by a gate instead"
        );
    }

    #[test]
    fn the_environment_hash_changes_when_a_setting_stops_being_readable() {
        let readable = workstation();
        let unreadable = Facts {
            turbo: Setting::Unreadable("windows does not expose the boost state".to_owned()),
            ..readable.clone()
        };

        let before = Capture::of(readable.clone(), idle(&readable));
        let after = Capture::of(unreadable.clone(), idle(&unreadable));

        assert_ne!(
            before.hash, after.hash,
            "two rows where different things were checked must not look interchangeable"
        );
    }

    #[test]
    fn a_machine_nobody_has_written_a_gate_set_for_produces_nothing() {
        let facts = Facts {
            os: "freebsd".to_owned(),
            arch: "riscv64".to_owned(),
            cpu: "something nobody has measured on".to_owned(),
            hypervisor: Hypervisor::None,
            ..workstation()
        };
        let conditions = idle(&facts);
        let capture = Capture::of(facts, conditions);

        assert_eq!(capture.class, Class::Unknown);
        assert_eq!(capture.permits(), Permit::Nothing);
    }

    #[test]
    fn every_class_has_a_ceiling_with_a_reason_on_it() {
        // The reason is what a person acts on, so an empty one is a bug even though nothing would
        // fail without this test.
        let facts = workstation();
        for class in [Class::A, Class::B, Class::C, Class::D, Class::Unknown] {
            let Ceiling { because, .. } = crate::policy::ceiling(class, &facts);
            assert!(!because.is_empty(), "{class} has no reason on its ceiling");
        }
    }
}
