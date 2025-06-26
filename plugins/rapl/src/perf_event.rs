use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::error::PollError,
    plugin::util::{CounterDiff, CounterDiffUpdate},
    resources::{Resource, ResourceConsumer},
};
use anyhow::{Context, Result};
use perf_event_open_sys as sys;
use std::{
    fs::{self, File},
    io::{self, Read},
    os::fd::FromRawFd,
    path::{Path, PathBuf},
};

use super::cpus::{self, CpuId};
use super::domains::RaplDomainType;

// See https://github.com/torvalds/linux/commit/4788e5b4b2338f85fa42a712a182d8afd65d7c58
// for an explanation of the RAPL PMU driver.

pub(crate) const PERF_MAX_ENERGY: u64 = u64::MAX;
pub(crate) const PERF_SYSFS_DIR: &str = "/sys/devices/power";
const PERMISSION_ADVICE: &str = "Try to set kernel.perf_event_paranoid to 0 or -1, or to give CAP_PERFMON to the application's binary (CAP_SYS_ADMIN before Linux 5.8).";

/// manages power events instantiations
pub struct PowerEventFactory;

/// describes power event metadata
#[derive(Debug, Clone, PartialEq)]
pub struct PowerEvent {
    /// The name of the power event, as reported by the sysfs. This corresponds to a RAPL **domain name**, like "pkg".
    pub name: String,
    /// The RAPL domain type, as an enum.
    pub domain: RaplDomainType,
    /// The event code to use as a "config" field for perf_event_open
    pub code: u8,
    /// should be "Joules"
    pub unit: String,
    /// The scale to apply in order to get joules (`energy_j = count * scale`).
    /// Should be "0x1.0p-32" (thus, f32 is fine)
    pub scale: f32,
}

/// manages power event counter collection
struct OpenedPowerEvent {
    fd: File,
    scale: f64,
    domain: RaplDomainType,
    resource: Resource,
    counter: CounterDiff,
}

/// Energy probe based on perf_event for intel RAPL.
pub struct PerfEventProbe {
    /// Id of the metric to push.
    metric: TypedMetricId<f64>,
    /// Ready-to-use power events with additional metadata.
    events: Vec<OpenedPowerEvent>,
}

/// Retrieves all RAPL power events from /sys/devices/power base path.
/// See all_power_events_from_path comments for more details
pub fn all_power_events() -> Result<Vec<PowerEvent>> {
    all_power_events_from_path(Path::new(PERF_SYSFS_DIR))
}

/// Retrieves all RAPL power events from a given base path (eg: /sys/devices/power)
/// There can be more than just `cores`, `pkg` and `dram`.
/// For instance, there can be `gpu` and
/// [`psys`](https://patchwork.kernel.org/project/linux-pm/patch/1458253409-13318-1-git-send-email-srinivas.pandruvada@linux.intel.com/).
pub fn all_power_events_from_path(base_path: &Path) -> Result<Vec<PowerEvent>> {
    let mut events: Vec<PowerEvent> = Vec::new();

    // Find all the events
    let power_event_dir = base_path.join("events");
    for e in fs::read_dir(&power_event_dir).context(format!(
        "Could not read {}. {PERMISSION_ADVICE}",
        power_event_dir
            .into_os_string()
            .into_string()
            .expect("error while converting power_event_dir to string")
    ))? {
        let entry = e?;
        let path = entry.path();
        if let Some(power_event) = PowerEventFactory::from_path(&path)? {
            events.push(power_event);
        }
    }
    Ok(events)
}

impl PowerEventFactory {
    /// creates a new PowerEvent from an event base path. In case the path is not identified as a RAPL event one, None will be returned.
    /// (eg: /sys/devices/power/events/energy-cores)
    pub fn from_path(base_path: &Path) -> anyhow::Result<Option<PowerEvent>> {
        let name = match Self::name_from_base_path(base_path)? {
            Some(name) => name,
            None => return Ok(None),
        };
        let code = Self::code_from_base_path(base_path)?;
        let unit = Self::unit_from_base_path(base_path)?;
        let scale = Self::scale_from_base_path(base_path)?;
        let domain = Self::domain_type_from_name(&name).with_context(|| format!("Unknown RAPL perf event {name}"))?;

        Ok(Some(PowerEvent {
            name,
            domain,
            code,
            unit,
            scale,
        }))
    }

    fn name_from_base_path(path: &Path) -> Result<Option<String>> {
        if !path.is_file() {
            return Ok(None);
        }

        let file_name = path
            .file_name()
            .with_context(|| format!("path has no file name: {:?}", path))?
            .to_string_lossy();

        if file_name.contains('.') {
            return Ok(None);
        }

        match file_name.strip_prefix("energy-") {
            Some(event_name) => Ok(Some(event_name.to_owned())),
            None => Ok(None),
        }
    }

    fn domain_type_from_name(name: &str) -> Option<RaplDomainType> {
        match name {
            "cores" => Some(RaplDomainType::PP0),
            "gpu" => Some(RaplDomainType::PP1),
            "psys" => Some(RaplDomainType::Platform),
            "pkg" => Some(RaplDomainType::Package),
            "ram" => Some(RaplDomainType::Dram),
            _ => None,
        }
    }

    fn code_from_base_path(path: &Path) -> Result<u8> {
        let read = fs::read_to_string(path).with_context(|| format!("Could not read {path:?}. {PERMISSION_ADVICE}"))?;
        let code_str = read
            .trim_end()
            .strip_prefix("event=0x")
            .with_context(|| format!("Failed to strip {path:?}: '{read}'"))?;
        let code = u8::from_str_radix(code_str, 16).with_context(|| format!("Failed to parse {path:?}: '{read}'"))?; // hexadecimal
        Ok(code)
    }

    fn unit_from_base_path(path: &Path) -> Result<String> {
        let mut path = path.to_path_buf();
        path.set_extension("unit");
        let unit_str = fs::read_to_string(path)?.trim_end().to_string();
        Ok(unit_str)
    }

    fn scale_from_base_path(path: &Path) -> Result<f32> {
        let mut path = path.to_path_buf();
        path.set_extension("scale");
        let read = fs::read_to_string(&path)?;
        let scale = read
            .trim_end()
            .parse()
            .with_context(|| format!("Failed to parse {path:?}: '{read}'"))?;
        Ok(scale)
    }
}

impl PowerEvent {
    /// Make a system call to [perf_event_open](https://www.man7.org/linux/man-pages/man2/perf_event_open.2.html)
    /// with `attr.config = self.code` and `attr.type = pmu_type`.
    ///
    /// # Arguments
    /// * `pmu_type` - The type of the RAPL PMU, given by [`pmu_type()`].
    /// * `cpu_id` - Defines which CPU (core) to monitor, given by [`super::cpus_to_monitor()`]
    ///
    pub fn perf_event_open(&self, pmu_type: u32, cpu_id: u32) -> std::io::Result<i32> {
        // Only some combination of (pid, cpu) are valid.
        // For RAPL PMU events, we use (-1, cpu) which means "all processes, one cpu".
        let pid = -1; // all processes
        let cpu = cpu_id as i32;

        let mut attr = sys::bindings::perf_event_attr::default();
        attr.config = self.code.into();
        attr.type_ = pmu_type;
        attr.size = core::mem::size_of_val(&attr) as u32;
        log::trace!("perf_event_open {attr:?}");

        let result = unsafe { sys::perf_event_open(&mut attr, pid, cpu, -1, 0) };
        if result == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(result)
        }
    }

    /// creates a new OpenedPowerEvent from Self by opening the file using file descriptor provided by perf_event
    fn open(&self, pmu_type: u32, cpu: u32, socket: u32) -> anyhow::Result<OpenedPowerEvent> {
        let raw_fd = self.perf_event_open(pmu_type, cpu)?;
        let fd = unsafe { File::from_raw_fd(raw_fd) };
        Ok(OpenedPowerEvent {
            fd,
            scale: self.scale as f64,
            domain: self.domain,
            resource: self.domain.to_resource(socket),
            counter: CounterDiff::with_max_value(PERF_MAX_ENERGY),
        })
    }
}

impl OpenedPowerEvent {
    fn read_counter_diff_in_joules(&mut self) -> anyhow::Result<Option<f64>> {
        match self.read_counter_diff()? {
            Some(diff) => Ok(Some((diff as f64) * self.scale)),
            None => Ok(None),
        }
    }

    fn read_counter_diff(&mut self) -> anyhow::Result<Option<u64>> {
        let counter_value = self.read_counter_value().context(format!(
            "failed to read perf_event {:?} for domain {:?}",
            self.fd, self.domain
        ))?;

        // correct any overflows
        Ok(match self.counter.update(counter_value) {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(diff) => Some(diff),
            CounterDiffUpdate::CorrectedDifference(diff) => {
                log::debug!("Overflow on perf_event counter for RAPL domain {}", self.domain);
                Some(diff)
            }
        })
    }

    fn read_counter_value(&mut self) -> io::Result<u64> {
        let mut buf = [0u8; 8];
        // rewind() is INVALID for perf events, we must read "at the cursor" every time
        let _ = self.fd.read(&mut buf)?;
        Ok(u64::from_ne_bytes(buf))
    }
}

impl PerfEventProbe {
    /// creates a new PerfEventProbe by passing an Alumet metric ID for energy measurement and related power events
    pub fn new(metric: TypedMetricId<f64>, power_events: &Vec<PowerEvent>) -> anyhow::Result<PerfEventProbe> {
        let all_cpus = cpus::online_cpus()?;
        let socket_cpus = cpus::cpus_to_monitor_with_perf()
        .context("I could not determine how to use perf_events to read RAPL energy counters. The Intel RAPL PMU module may not be enabled, is your Linux kernel too old?")?;

        let n_sockets = socket_cpus.len();
        let n_cpu_cores = all_cpus.len();
        log::debug!("{n_sockets}/{n_cpu_cores} monitorable CPU (cores) found: {socket_cpus:?}");

        // Build the right combination of perf events.
        let mut events_on_cpus = Vec::new();
        for event in power_events {
            for cpu in &socket_cpus {
                events_on_cpus.push((event, cpu));
            }
        }
        log::debug!("Events to read: {events_on_cpus:?}");

        match pmu_type() {
            Ok(pmu_type) => {
                let mut opened = Vec::with_capacity(events_on_cpus.len());
                for (event, CpuId { cpu, socket }) in events_on_cpus {
                    match event.open(pmu_type, *cpu, *socket) {
                        Ok(opened_zone) => opened.push(opened_zone),
                        Err(e) => {
                            Self::handle_insufficient_privileges(&e);
                            return Err(e);
                        }
                    }
                }
                Ok(PerfEventProbe { metric, events: opened })
            }
            Err(e) => {
                Self::handle_insufficient_privileges(&e);
                Err(e)
            }
        }
    }

    fn handle_insufficient_privileges(e: &anyhow::Error) {
        fn resolve_application_path() -> std::io::Result<PathBuf> {
            std::env::current_exe()?.canonicalize()
        }
        let app_path = resolve_application_path()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_owned()))
            .unwrap_or(String::from("path/to/agent"));
        let msg = indoc::formatdoc! {"
            I could not use perf_events to read RAPL energy counters: {e}.
            This warning is probably caused by insufficient privileges.
            To fix this, you have 3 possibilities:
            1. Grant the CAP_PERFMON (CAP_SYS_ADMIN on Linux < 5.8) capability to the agent binary.
                sudo setcap cap_perfmon=ep \"{app_path}\"        
                Note: to grant multiple capabilities to the binary, you must put all the capabilities in the same command.
                sudo setcap \"cap_sys_nice+ep cap_perfmon=ep\" \"{app_path}\" 
                    
            2. Change a kernel setting to allow every process to read the perf_events.
                sudo sysctl -w kernel.perf_event_paranoid=0
                    
            3. Run the agent as root (not recommanded)."};
        log::warn!("{msg}");
    }
}

impl alumet::pipeline::Source for PerfEventProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut pkg_total = 0.0;
        for evt in &mut self.events {
            // read the new value of the perf-events counter
            if let Some(joules) = evt.read_counter_diff_in_joules()? {
                let consumer = ResourceConsumer::LocalMachine;
                measurements.push(
                    MeasurementPoint::new(timestamp, self.metric, evt.resource.clone(), consumer, joules)
                        .with_attr("domain", evt.domain.as_str()),
                );
                if matches!(evt.resource, Resource::CpuPackage { id: _ }) {
                    pkg_total += joules;
                }
            }
            // NOTE: the energy can be a floating-point number in Joules,
            // without any loss of precision. Why? Because multiplying any number
            // by a float that is a power of two will only change the "exponent" part,
            // not the "mantissa", and the energy unit for RAPL is always a power of two.
            //
            // A f32 can hold integers without any precision loss
            // up to approximately 2^24, which is not enough for the RAPL counter values,
            // so we use a f64 here.
        }
        if pkg_total != 0.0 {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metric,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                pkg_total,
            ).with_attr("domain", "package_total"));
        }
        Ok(())
    }
}

/// Retrieves the type of the RAPL PMU (Power Monitoring Unit) in the Linux kernel.
fn pmu_type() -> Result<u32> {
    pmu_type_from_path(Path::new("/sys/devices/power/type"))
}

/// Retrieves the type of the RAPL PMU (Power Monitoring Unit) in the Linux kernel.
fn pmu_type_from_path(path: &Path) -> Result<u32> {
    let read = fs::read_to_string(path).with_context(|| format!("Failed to read {path:?}"))?;
    let typ = read
        .trim_end()
        .parse()
        .with_context(|| format!("Failed to parse {path:?}: '{read}'"))?;
    Ok(typ)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_mock::*;

    use nix::unistd::write;
    use std::os::fd::OwnedFd;
    use tempfile::tempdir;

    #[cfg(test)]
    #[test]
    fn test_pmu_type() -> anyhow::Result<()> {
        let tmp = tempdir()?;
        let base_path = tmp.keep();

        use EntryType::*;
        let pmu_type_entry = Entry {
            path: "pmu_type",
            entry_type: File("32"),
        };
        create_mock_layout(base_path.clone(), &[pmu_type_entry])?;
        let actual = pmu_type_from_path(&base_path.join("pmu_type"))?;
        let expected = 32;

        assert_eq!(actual, expected);

        Ok(())
    }

    #[cfg(test)]
    #[test]
    fn test_open_all() -> anyhow::Result<()> {
        let tmp = tempdir()?;
        let base_path = tmp.keep();

        use EntryType::*;
        let perf_event_entries = [
            Entry {
                path: "events",
                entry_type: Dir,
            },
            Entry {
                path: "events/energy-cores",
                entry_type: File("event=0x01"),
            },
            Entry {
                path: "events/energy-cores.scale",
                entry_type: File("2.3283064365386962890625e-10"),
            },
            Entry {
                path: "events/energy-cores.unit",
                entry_type: File("Joules"),
            },
            Entry {
                path: "events/energy-pkg",
                entry_type: File("event=0x02"),
            },
            Entry {
                path: "events/energy-pkg.scale",
                entry_type: File("2.3283064365386962890625e-10"),
            },
            Entry {
                path: "events/energy-pkg.unit",
                entry_type: File("Joules"),
            },
            Entry {
                path: "events/energy-psys",
                entry_type: File("event=0x05"),
            },
            Entry {
                path: "events/energy-psys.scale",
                entry_type: File("2.3283064365386962890625e-10"),
            },
            Entry {
                path: "events/energy-psys.unit",
                entry_type: File("Joules"),
            },
        ];

        create_mock_layout(base_path.clone(), &perf_event_entries)?;

        let mut actual_power_events = all_power_events_from_path(base_path.as_path())?;

        let mut expected_power_events = vec![
            PowerEvent {
                name: "psys".to_string(),
                domain: RaplDomainType::Platform,
                code: 5,
                unit: "Joules".to_string(),
                scale: 2.3283064365386962890625e-10,
            },
            PowerEvent {
                name: "pkg".to_string(),
                domain: RaplDomainType::Package,
                code: 2,
                unit: "Joules".to_string(),
                scale: 2.3283064365386962890625e-10,
            },
            PowerEvent {
                name: "cores".to_string(),
                domain: RaplDomainType::PP0,
                code: 1,
                unit: "Joules".to_string(),
                scale: 2.3283064365386962890625e-10,
            },
        ];

        actual_power_events.sort_by_key(|e: &PowerEvent| e.name.clone());
        expected_power_events.sort_by_key(|e: &PowerEvent| e.name.clone());

        assert_eq!(actual_power_events, expected_power_events);

        Ok(())
    }

    fn fake_opened_power_event() -> (OpenedPowerEvent, OwnedFd) {
        use nix::unistd::pipe;
        use std::os::fd::IntoRawFd;
        use std::os::unix::io::FromRawFd;

        let (read_fd, write_fd) = pipe().unwrap();

        let file = unsafe { File::from_raw_fd(read_fd.into_raw_fd()) };
        (
            OpenedPowerEvent {
                fd: file,
                scale: 2.3283064365386962890625e-10,
                domain: RaplDomainType::Package, // dummy value
                resource: RaplDomainType::Package.to_resource(0),
                counter: CounterDiff::with_max_value(PERF_MAX_ENERGY),
            },
            write_fd,
        )
    }

    #[cfg(test)]
    #[test]
    fn test_opened_power_event() -> anyhow::Result<()> {
        let (mut opened_power_event, write_fd) = fake_opened_power_event();

        let val1 = 42u64.to_ne_bytes();
        write(&write_fd, &val1)?;
        let value = opened_power_event.read_counter_value()?;
        assert_eq!(value, 42);

        let val2 = 43u64.to_ne_bytes();
        write(&write_fd, &val2)?;
        let value = opened_power_event.read_counter_value()?;
        assert_eq!(value, 43);

        let val3 = 44u64.to_ne_bytes();
        write(&write_fd, &val3)?;
        let value = opened_power_event.read_counter_diff()?;
        assert_eq!(value, None);

        let val4 = 45u64.to_ne_bytes();
        write(&write_fd, &val4)?;
        let value = opened_power_event.read_counter_diff()?;
        assert_eq!(value, Some(1));

        let val5 = 4294967341u64.to_ne_bytes();
        write(&write_fd, &val5)?;
        let value = opened_power_event.read_counter_diff_in_joules()?;
        assert_eq!(value, Some(1.0));

        Ok(())
    }
}
