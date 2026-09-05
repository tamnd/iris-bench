//! Which of the hardware classes in `docs/MACHINES.md` this machine is.
//!
//! The class is worked out from what the machine reports about itself, never from its name. A
//! hostname is not a fact about hardware, it does not survive the machine being rebuilt, and it is
//! the sort of thing that ends up in a public result table by accident. The processor, the presence
//! of a hypervisor and the operating system are all things a reader can check against the machine
//! notes, so those are what the classification reads.
//!
//! A machine that matches no class is [`Class::Unknown`], and unknown produces nothing publishable.
//! That is deliberately the safe direction: somebody running the harness on a laptop that nobody has
//! written a gate set for should get a refusal, not a number.

use serde::{Deserialize, Serialize};

use crate::facts::{Facts, Hypervisor};

/// A hardware class from the machine notes.
///
/// The letters are the document's own vocabulary rather than an abbreviation invented here, so that
/// a class in a result row and a class in `docs/MACHINES.md` are obviously the same thing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Class {
    /// Virtualised AMD EPYC servers, shared tenancy.
    A,
    /// The Intel Core i9-13900K workstation.
    B,
    /// Hosted arm64 Linux.
    C,
    /// macOS on Apple silicon, the development machine.
    D,
    /// Something nobody has written a gate set for.
    Unknown,
}

impl Class {
    /// The class as the machine notes describe it, for an error a person has to act on.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::A => "class A, a virtualised AMD EPYC server on shared tenancy",
            Self::B => "class B, the Intel Core i9-13900K workstation",
            Self::C => "class C, a hosted arm64 Linux runner",
            Self::D => "class D, the macOS development machine",
            Self::Unknown => "an unclassified machine",
        }
    }
}

impl std::fmt::Display for Class {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.description())
    }
}

/// Works out which class a machine belongs to from what it reports about itself.
///
/// The order of the checks matters in one place. A class B machine running under WSL2 is still class
/// B, because it is the same silicon and the machine notes treat WSL2 as a caveat on what its
/// numbers mean rather than as a different machine. So the processor is examined before the
/// hypervisor is, and the hypervisor only decides the class for the EPYC guests, where it is the
/// whole reason they are a separate class.
#[must_use]
pub fn classify(facts: &Facts) -> Class {
    if facts.os == "macos" && facts.arch == "aarch64" {
        return Class::D;
    }
    if facts.cpu.contains("i9-13900K") {
        return Class::B;
    }
    if facts.cpu.contains("EPYC") && facts.hypervisor != Hypervisor::None {
        return Class::A;
    }
    if facts.os == "linux" && facts.arch == "aarch64" {
        return Class::C;
    }
    Class::Unknown
}
