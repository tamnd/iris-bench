//! The gate set for each hardware class, and the most each class can ever produce.
//!
//! This is the file to read to find out why a machine was refused, and the file to change if the
//! answer is wrong. Everything else in the crate reads a machine or reports a decision. The decision
//! itself is here.
//!
//! The four classes are the ones in `docs/MACHINES.md` and the reasoning is that document's rather
//! than this one's. Where a rule here looks arbitrary, the machine notes say why it is not.

use crate::class::Class;
use crate::conditions::Conditions;
use crate::facts::{Facts, Hypervisor};
use crate::gate::{Ceiling, Gate, Permit};

/// How much of the fitted memory has to still be available.
///
/// A quarter. Not because a measurement needs a quarter of the machine's memory, but because a
/// machine with less than that free has something substantial resident in it, and whatever that is
/// has a working set competing for the same cache.
const MEMORY_HEADROOM: f64 = 0.25;

/// The floor under that fraction, in bytes.
///
/// A quarter of a small machine is not much, and the corpora here are measured in gigabytes.
const MEMORY_FLOOR: u64 = 2 * 1024 * 1024 * 1024;

/// The most load per logical processor that still counts as an idle machine.
///
/// A fifth. A completely idle Linux box sits near zero and the housekeeping that wakes up on a timer
/// does not reach this. One busy thread on an eight processor machine does.
const LOAD_PER_CPU: f64 = 0.2;

/// What the class can produce at best, and why it is capped there.
///
/// Two classes are capped by what they are: a shared tenancy guest cannot produce a duration however
/// idle it looks, and the development machine is not a measurement machine. The workstation is
/// capped by how much of it can be read, which is the rule that matters most here. Where the
/// governor, the boost state and the processor affinity cannot be read, they have not been checked,
/// and a machine nobody checked produces ratios rather than durations. Being unable to see a setting
/// is not evidence that the setting is right.
#[must_use]
pub fn ceiling(class: Class, facts: &Facts) -> Ceiling {
    match class {
        Class::A => Ceiling::new(
            Permit::Nothing,
            "the guest cannot set the governor, cannot disable boost, cannot pin to a physical core \
             and cannot see what a neighbouring tenant is doing",
        ),
        Class::B => workstation_ceiling(facts),
        Class::C => Ceiling::new(
            Permit::Ratios,
            "a shared hosted runner has a run to run spread of five to fifteen percent, which is \
             larger than most effects worth reporting",
        ),
        Class::D => Ceiling::new(
            Permit::Nothing,
            "this is the development machine, which exists to check that the harness builds and \
             runs",
        ),
        Class::Unknown => Ceiling::new(
            Permit::Nothing,
            "nobody has written a gate set for this machine, so nothing about it has been checked",
        ),
    }
}

/// The workstation is the one class whose ceiling depends on what can be read rather than on what it
/// is.
fn workstation_ceiling(facts: &Facts) -> Ceiling {
    if facts.hypervisor == Hypervisor::Wsl {
        return Ceiling::new(
            Permit::Ratios,
            "there is a hypervisor with memory ballooning underneath, which is fine for a ratio \
             taken inside one run and is a known unknown for an absolute number",
        );
    }

    let unreadable: Vec<&str> = [
        ("the frequency governor", &facts.governor),
        ("the boost state", &facts.turbo),
        ("the processor affinity", &facts.affinity),
    ]
    .into_iter()
    .filter(|(_, setting)| setting.reading().is_none())
    .map(|(name, _)| name)
    .collect();

    if unreadable.is_empty() {
        Ceiling::new(
            Permit::Durations,
            "everything that keeps the clock steady on this machine can be read and has been checked",
        )
    } else {
        Ceiling::new(
            Permit::Ratios,
            format!(
                "{} cannot be read here, so nothing has checked whether the clock is steady",
                unreadable.join(", ")
            ),
        )
    }
}

/// Every gate that can be evaluated on this machine, in the order they are worth reading.
///
/// A setting that could not be read produces no gate at all rather than a third kind of outcome. The
/// reason is on [`crate::Outcome`]: an outcome between passing and failing has to be interpreted by
/// every caller, and they will not all interpret it the same way. What the unreadable setting does
/// instead is lower the ceiling and change the environment hash.
#[must_use]
pub fn gates(class: Class, facts: &Facts, conditions: &Conditions) -> Vec<Gate> {
    let mut gates = vec![
        memory_headroom(facts, conditions),
        busy_processes(conditions),
    ];

    if let Some(load) = conditions.load_average {
        gates.push(load_average(facts, load));
    }

    if class == Class::B {
        gates.extend(workstation_gates(facts));
    }

    gates
}

/// The gates that only apply to the workstation, and only where the setting is readable.
fn workstation_gates(facts: &Facts) -> Vec<Gate> {
    let mut gates = Vec::new();

    if let Some(governor) = facts.governor.reading() {
        // Two different settings under one name, because they answer the same question. The Linux
        // reading is a cpufreq governor and the Windows reading is a power scheme, and the wanted
        // string says which one is being asked for so that the refusal is actionable on the machine
        // it came from.
        //
        // Which one it is comes from the facts and not from the target this binary was built for.
        // In a live run those are the same thing, so the distinction costs nothing and buys the
        // property that a capture means the same whoever reads it back.
        if facts.os == "windows" {
            let steady = governor.contains("High performance") || governor.contains("Ultimate");
            gates.push(Gate::check(
                "power-scheme",
                steady,
                format!("the {governor} power scheme"),
                "the High performance or Ultimate Performance scheme, so that the clock does not \
                 drop between samples",
            ));
        } else {
            gates.push(Gate::check(
                "frequency-governor",
                governor == "performance",
                format!("the {governor} governor"),
                "the performance governor, so that the clock does not drop between samples",
            ));
        }
    }

    if let Some(turbo) = facts.turbo.reading() {
        gates.push(Gate::check(
            "turbo",
            turbo == "off",
            format!("boost {turbo}"),
            "boost off, so that the clock does not fall away as the part warms up",
        ));
    }

    if let Some(affinity) = facts.affinity.reading() {
        // This is a hybrid part. What is being checked is that somebody restricted the run at all,
        // because a run free to move between a performance core and an efficiency core produces a
        // distribution with two modes, and a confidence interval over two modes describes neither.
        // Which processors are the performance ones is not something this can work out portably, so
        // the gate checks that a choice was made and the capture records which choice it was.
        let everything = format!("0-{}", facts.logical_cpus.saturating_sub(1));
        gates.push(Gate::check(
            "core-pinning",
            affinity != everything && !affinity.is_empty(),
            format!("this run may use processors {affinity}"),
            "a run pinned to the performance cores rather than free to move across all of them",
        ));
    }

    gates
}

/// Enough memory left that nothing substantial is resident alongside the measurement.
fn memory_headroom(facts: &Facts, conditions: &Conditions) -> Gate {
    let available = conditions.available_memory_bytes;
    let wanted = fraction_of(facts.memory_bytes, MEMORY_HEADROOM).max(MEMORY_FLOOR);

    Gate::check(
        "memory-headroom",
        available >= wanted,
        format!("{} available", gibibytes(available)),
        format!("at least {} available", gibibytes(wanted)),
    )
}

/// Nothing else on the machine is doing real work.
fn busy_processes(conditions: &Conditions) -> Gate {
    Gate::check(
        "busy-processes",
        conditions.busy_processes == 0,
        format!(
            "{} other processes using a processor",
            conditions.busy_processes
        ),
        "nothing else running",
    )
}

/// The same question as the busy process gate, asked of the kernel's own summary.
///
/// Both are kept. The process walk sees a single busy process on an otherwise idle machine that the
/// load average has not caught up with yet, and the load average sees a machine that is thrashing in
/// a way no single process accounts for.
fn load_average(facts: &Facts, load: f64) -> Gate {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a processor count large enough to lose precision in an f64 does not exist"
    )]
    let wanted = LOAD_PER_CPU * facts.logical_cpus as f64;

    Gate::check(
        "load-average",
        load <= wanted,
        format!("a one minute load average of {load:.2}"),
        format!("a one minute load average no higher than {wanted:.2}"),
    )
}

/// A fraction of a byte count, without going through a lossy conversion in the common direction.
fn fraction_of(bytes: u64, fraction: f64) -> u64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a memory size large enough to lose precision in an f64 does not exist"
    )]
    let scaled = bytes as f64 * fraction;

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a fraction between zero and one of a u64 is in range and not negative"
    )]
    let rounded = scaled as u64;

    rounded
}

/// Byte counts as a person would say them out loud.
fn gibibytes(bytes: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a memory size large enough to lose precision in an f64 does not exist"
    )]
    let gib = bytes as f64 / (1024.0 * 1024.0 * 1024.0);

    format!("{gib:.1} GiB")
}
