//! Gates, and what a machine is allowed to produce once they have been evaluated.
//!
//! # A gate either aborts or it is not a gate
//!
//! There is no warning here and there is no override. A number produced on a machine that failed a
//! gate is a number with a caveat attached, and a caveat attached to a number does not survive being
//! copied into a slide. The only way to make the caveat stick is to not produce the number.
//!
//! # A gate exists only where it can be evaluated
//!
//! The frequency governor is readable on Linux and is not readable on macOS. The tempting move is a
//! third outcome, something like unknown, that sits between passing and failing. That turns out to
//! be the worst option available: a caller has to decide what unknown means, every caller decides
//! differently, and the decision is made far away from the person who knew why the reading was
//! missing.
//!
//! So a gate set is per class and per platform, and a class only carries the gates its platform can
//! answer. What happens to the unreadable setting is that it is recorded as unreadable in the
//! environment capture, and the capture is hashed into every result row. Two rows where different
//! things were checkable end up with different environment hashes and cannot be silently compared.
//! The protection moves from a runtime decision nobody sees to a field in the data everybody sees.

use serde::{Deserialize, Serialize};

use crate::class::Class;

/// What a machine may be used to produce.
///
/// Ordered, weakest first, so a caller can ask whether what a machine permits is at least what a run
/// needs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Permit {
    /// Nothing that gets published. Correctness work and digests, which do not have a clock in them.
    Nothing,
    /// Ratios taken inside one run, where the machine is its own control.
    Ratios,
    /// Absolute durations, which is the only thing that can be compared across machines.
    Durations,
}

impl Permit {
    /// How to say this in a sentence about a refusal.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Nothing => "no published measurement",
            Self::Ratios => "within run ratios only",
            Self::Durations => "absolute durations",
        }
    }
}

impl std::fmt::Display for Permit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.description())
    }
}

/// The result of evaluating one gate.
///
/// Deliberately not marked as open to further variants. The doctrine at the top of this file is that
/// there is no third outcome, and leaving room for one invites a caller to write the wildcard arm
/// that quietly treats a future unknown as a pass. Adding a variant here should break every match
/// that reads one, because every one of them would need rethinking.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum Outcome {
    /// The machine is in the state the gate wanted.
    Pass {
        /// What was read, so that a passing capture is still a record of what was true.
        observed: String,
    },
    /// It is not, and the run stops.
    Fail {
        /// What was read.
        observed: String,
        /// What would have passed, phrased so a person can go and change it.
        wanted: String,
    },
}

impl Outcome {
    /// Whether this outcome lets a run continue.
    #[must_use]
    pub const fn passed(&self) -> bool {
        matches!(self, Self::Pass { .. })
    }
}

/// One named check against the machine.
///
/// The name is what appears in the refusal, so it is written as the thing being checked rather than
/// as a sentence about the check. A person reading `frequency-governor` in an aborted run knows
/// where to go.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Gate {
    /// The identifier used in the refusal message and in the capture.
    pub name: String,
    /// What happened when it was evaluated.
    pub outcome: Outcome,
}

impl Gate {
    /// A gate that passed, recording what was seen.
    #[must_use]
    pub fn pass(name: impl Into<String>, observed: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            outcome: Outcome::Pass {
                observed: observed.into(),
            },
        }
    }

    /// A gate that failed, recording what was seen and what would have passed.
    #[must_use]
    pub fn fail(
        name: impl Into<String>,
        observed: impl Into<String>,
        wanted: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            outcome: Outcome::Fail {
                observed: observed.into(),
                wanted: wanted.into(),
            },
        }
    }

    /// Builds a pass or a fail from a condition, which is how nearly every gate here is written.
    #[must_use]
    pub fn check(
        name: impl Into<String>,
        ok: bool,
        observed: impl Into<String>,
        wanted: impl Into<String>,
    ) -> Self {
        if ok {
            Self::pass(name, observed)
        } else {
            Self::fail(name, observed, wanted)
        }
    }
}

/// The most a machine can produce, and why it is capped there.
///
/// The reason is carried rather than recomputed for a message, because it is the half a person acts
/// on. Knowing that a machine produces ratios is not useful. Knowing that it produces ratios because
/// the boost state cannot be read is a thing somebody can go and fix.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Ceiling {
    /// The most this machine can produce.
    pub permit: Permit,
    /// Why it is capped there, as a clause that reads after the word because.
    pub because: String,
}

impl Ceiling {
    /// Names a ceiling and the reason for it.
    #[must_use]
    pub fn new(permit: Permit, because: impl Into<String>) -> Self {
        Self {
            permit,
            because: because.into(),
        }
    }
}

/// Why a run is not allowed to produce what it asked for.
///
/// Both variants name something specific. A refusal that says the machine is unsuitable and stops
/// there is one a person works around by disabling the check.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Refusal {
    /// A gate failed.
    #[error("the {gate} gate failed: this machine reports {observed}, and {asked} needs {wanted}")]
    Gate {
        /// Which gate.
        gate: String,
        /// What it read.
        observed: String,
        /// What would have passed.
        wanted: String,
        /// What the run had asked to produce.
        asked: Permit,
    },
    /// Every gate passed, but this machine does not produce what was asked for.
    #[error(
        "{asked} were asked for, and this is {class}, which produces {permits} because {because}"
    )]
    Ceiling {
        /// The class it was identified as.
        class: Class,
        /// The most it can produce.
        permits: Permit,
        /// Why it is capped there.
        because: String,
        /// What the run had asked to produce.
        asked: Permit,
    },
}
