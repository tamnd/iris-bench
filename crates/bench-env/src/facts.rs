//! What a machine is, and how it is configured.
//!
//! Everything here is stable for as long as the machine is not reconfigured, which is what makes it
//! safe to hash. Anything that moves while a run is in progress belongs in [`crate::Conditions`]
//! instead.
//!
//! # Readings that are not available
//!
//! Every setting is a [`Setting`], which is either a reading or a note saying why there is not one.
//! The note is not a placeholder to be tidied up later. It is the record that nobody checked, it
//! goes into the environment hash, and it is what stops a row from a platform where the governor is
//! readable being compared against a row from a platform where it is not.

use std::fs;
#[cfg(target_os = "windows")]
use std::process::Command;

use serde::{Deserialize, Serialize};

/// A setting that some platforms report and others do not.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "detail")]
#[non_exhaustive]
pub enum Setting {
    /// What the machine said.
    Reading(String),
    /// Why there is no reading, phrased so that a person can tell a missing feature from a missing
    /// permission.
    Unreadable(String),
}

impl Setting {
    /// The reading, if there is one.
    #[must_use]
    pub fn reading(&self) -> Option<&str> {
        match self {
            Self::Reading(text) => Some(text),
            Self::Unreadable(_) => None,
        }
    }

    /// How to say this inside a sentence about a gate.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Reading(text) => text.clone(),
            Self::Unreadable(why) => format!("no reading, because {why}"),
        }
    }
}

/// Whether there is a hypervisor underneath, and which sort.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "detail")]
#[non_exhaustive]
pub enum Hypervisor {
    /// Nothing detected. On a platform where detection works, this means bare metal.
    None,
    /// The Windows Subsystem for Linux, which is a hypervisor with memory ballooning under it.
    Wsl,
    /// A guest of the named virtual machine monitor.
    Guest(String),
    /// This platform is not probed for one.
    Unknown,
}

/// What a machine is, in the form that gets hashed into every result row.
///
/// The field order is the serialisation order and the serialisation is what gets hashed, so
/// reordering these fields changes every environment hash ever produced. That is not a reason never
/// to do it, it is a reason to bump [`bench_core::METHODOLOGY_VERSION`] when it happens.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub struct Facts {
    /// The target architecture this binary was built for.
    pub arch: String,
    /// The operating system this binary was built for.
    pub os: String,
    /// The operating system version, as the system reports it.
    pub os_version: Setting,
    /// The kernel version, as the system reports it.
    pub kernel: Setting,
    /// The processor brand string.
    pub cpu: String,
    /// How many logical processors there are.
    pub logical_cpus: usize,
    /// How many physical cores there are, where the platform will say.
    pub physical_cores: Option<usize>,
    /// Total memory fitted, in bytes.
    pub memory_bytes: u64,
    /// Whether there is a hypervisor underneath.
    pub hypervisor: Hypervisor,
    /// The frequency governor on Linux, or the active power scheme on Windows.
    pub governor: Setting,
    /// Whether opportunistic boost is enabled.
    pub turbo: Setting,
    /// Which logical processors this process is allowed to run on.
    pub affinity: Setting,
}

impl Facts {
    /// Reads everything this platform will say about itself.
    ///
    /// Nothing here fails. A reading that cannot be taken becomes a [`Setting::Unreadable`] with the
    /// reason in it, because a capture that refuses to be taken tells a person less than a capture
    /// that says what it could not see.
    #[must_use]
    pub fn read() -> Self {
        let system = sysinfo::System::new_all();
        let cpus = system.cpus();

        Self {
            arch: std::env::consts::ARCH.to_owned(),
            os: std::env::consts::OS.to_owned(),
            os_version: from_option(
                sysinfo::System::os_version(),
                "the system did not report an operating system version",
            ),
            kernel: from_option(
                sysinfo::System::kernel_version(),
                "the system did not report a kernel version",
            ),
            cpu: cpus
                .first()
                .map_or_else(|| "unknown".to_owned(), |cpu| cpu.brand().trim().to_owned()),
            logical_cpus: cpus.len(),
            physical_cores: sysinfo::System::physical_core_count(),
            memory_bytes: system.total_memory(),
            hypervisor: hypervisor(),
            governor: governor(),
            turbo: turbo(),
            affinity: affinity(),
        }
    }
}

/// Turns an optional reading into a [`Setting`] with a stated reason when it is missing.
fn from_option(value: Option<String>, why: &str) -> Setting {
    value.map_or_else(
        || Setting::Unreadable(why.to_owned()),
        |text| Setting::Reading(text.trim().to_owned()),
    )
}

/// Reads a sysfs file, trimming the trailing newline every one of them has.
fn sysfs(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// Detects a hypervisor on Linux and does not guess anywhere else.
///
/// The order matters. WSL is reported as WSL rather than as a Hyper-V guest, because the machine
/// notes have a specific caveat about it that a generic guest does not carry.
fn hypervisor() -> Hypervisor {
    if cfg!(not(target_os = "linux")) {
        return Hypervisor::Unknown;
    }

    if let Some(release) = sysfs("/proc/sys/kernel/osrelease") {
        let lowered = release.to_lowercase();
        if lowered.contains("microsoft") || lowered.contains("wsl") {
            return Hypervisor::Wsl;
        }
    }
    if let Some(kind) = sysfs("/sys/hypervisor/type") {
        return Hypervisor::Guest(kind);
    }
    // The board vendor is what a virtual machine monitor puts its own name in. On real hardware this
    // is the name of whoever made the motherboard, which is not a hypervisor and not a machine
    // identity either.
    if let Some(vendor) = sysfs("/sys/class/dmi/id/sys_vendor") {
        const MONITORS: &[&str] = &[
            "QEMU",
            "Xen",
            "VMware",
            "Microsoft Corporation",
            "innotek",
            "Parallels",
            "Amazon EC2",
            "Google",
            "Alibaba",
            "OpenStack",
        ];
        if MONITORS.iter().any(|monitor| vendor.contains(monitor)) {
            return Hypervisor::Guest(vendor);
        }
    }
    Hypervisor::None
}

/// The frequency governor on Linux, the active power scheme on Windows, nothing on macOS.
///
/// These are not the same thing and the capture does not pretend they are. They occupy one field
/// because they are the same question, which is whether the operating system has been told to keep
/// the clock steady, and the reading itself says which one was asked.
fn governor() -> Setting {
    #[cfg(target_os = "linux")]
    {
        return sysfs("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").map_or_else(
            || {
                Setting::Unreadable(
                    "this kernel exposes no cpufreq governor for the first processor".to_owned(),
                )
            },
            Setting::Reading,
        );
    }

    #[cfg(target_os = "windows")]
    {
        return active_power_scheme();
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Setting::Unreadable(format!(
            "{} exposes no frequency governor",
            std::env::consts::OS
        ))
    }
}

/// The name of the active Windows power scheme, from `powercfg`.
///
/// `powercfg /getactivescheme` prints one line holding a GUID and then the scheme name in brackets.
/// The name is what a person recognises and the GUID is what varies between Windows installations
/// for the same scheme, so the name is what is kept.
#[cfg(target_os = "windows")]
fn active_power_scheme() -> Setting {
    let output = match Command::new("powercfg").arg("/getactivescheme").output() {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            return Setting::Unreadable(format!("powercfg exited with {}", output.status));
        }
        Err(error) => return Setting::Unreadable(format!("powercfg could not be run: {error}")),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    match (text.find('('), text.rfind(')')) {
        (Some(open), Some(close)) if open < close => {
            Setting::Reading(text[open + 1..close].trim().to_owned())
        }
        _ => Setting::Unreadable("powercfg printed no scheme name in brackets".to_owned()),
    }
}

/// Whether opportunistic boost is on.
///
/// Two different files depending on the driver, and neither exists on a machine whose kernel does
/// not let the setting be changed at all, which is itself the answer for a virtual machine.
fn turbo() -> Setting {
    #[cfg(target_os = "linux")]
    {
        if let Some(no_turbo) = sysfs("/sys/devices/system/cpu/intel_pstate/no_turbo") {
            return Setting::Reading(if no_turbo == "1" { "off" } else { "on" }.to_owned());
        }
        if let Some(boost) = sysfs("/sys/devices/system/cpu/cpufreq/boost") {
            return Setting::Reading(if boost == "1" { "on" } else { "off" }.to_owned());
        }
        return Setting::Unreadable(
            "this kernel exposes neither the intel_pstate turbo switch nor the cpufreq boost switch"
                .to_owned(),
        );
    }

    #[cfg(not(target_os = "linux"))]
    {
        Setting::Unreadable(format!(
            "{} does not expose the boost state to an unprivileged process",
            std::env::consts::OS
        ))
    }
}

/// Which logical processors this process may run on.
///
/// Read rather than assumed, which is the whole point on a hybrid part. A run that is free to move
/// between a performance core and an efficiency core produces a distribution with two modes, and a
/// confidence interval over two modes describes neither of them.
fn affinity() -> Setting {
    #[cfg(target_os = "linux")]
    {
        let Some(status) = sysfs("/proc/self/status") else {
            return Setting::Unreadable("this process has no status file".to_owned());
        };
        return status
            .lines()
            .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
            .map_or_else(
                || {
                    Setting::Unreadable(
                        "the process status file lists no allowed processors".to_owned(),
                    )
                },
                |list| Setting::Reading(list.trim().to_owned()),
            );
    }

    #[cfg(not(target_os = "linux"))]
    {
        Setting::Unreadable(format!(
            "reading the processor affinity of a running process is not implemented for {}",
            std::env::consts::OS
        ))
    }
}
