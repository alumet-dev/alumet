use std::time::Duration;

use alumet::{
    pipeline::trigger::TriggerSpec,
    plugin::rust::{deserialize_config, serialize_config, AlumetPlugin},
    units::{PrefixedUnit, Unit},
};
use anyhow::Context;
use procfs::{Current, CurrentSI};
use regex::Regex;
use serde::{Deserialize, Serialize};

mod kernel;
mod memory;
mod process;
mod serde_regex;

pub struct ProcfsPlugin {
    config: Option<Config>,
}

impl AlumetPlugin for ProcfsPlugin {
    fn name() -> &'static str {
        "procfs"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        Ok(Box::new(ProcfsPlugin { config: Some(config) }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        // TODO allow to add a source in a "paused" state for later activation?
        // Start kernel and meminfo sources, if enabled
        let config = self.config.take().unwrap();
        if config.kernel.enabled {
            let trigger = TriggerSpec::at_interval(config.kernel.poll_interval);
            let metrics = kernel::KernelMetrics::new(alumet).context("unable to register metrics for kernel probe")?;
            let source = kernel::KernelStatsProbe::new(metrics, procfs::KernelStats::PATH)
                .context("unable to create kernel probe")?;
            alumet.add_source(Box::new(source), trigger);
        }
        if config.memory.enabled {
            let trigger = TriggerSpec::at_interval(config.memory.poll_interval);
            let metrics: anyhow::Result<Vec<_>> = config
                .memory
                .metrics
                .into_iter()
                .map(|procfs_entry_name| {
                    let metric_name = convert_to_snake_case(&procfs_entry_name);
                    let metric = alumet
                        .create_metric(&metric_name, PrefixedUnit::kilo(Unit::Byte), "?")
                        .with_context(|| format!("unable to register metric {metric_name} for memory probe"))?;
                    Ok((procfs_entry_name, metric))
                })
                .collect();
            let source =
                memory::MeminfoProbe::new(metrics?, procfs::Meminfo::PATH).context("unable to create memory probe")?;
            alumet.add_source(Box::new(source), trigger);
        }
        if config.processes.enabled {
            let metrics = process::ProcessMetrics {
                metric_cpu_time: alumet
                    .create_metric("process_cpu_time", PrefixedUnit::milli(Unit::Second), "CPU usage")
                    .context("unable to register metric cpu_time for process probe")?,
                metric_memory: alumet
                    .create_metric("process_memory", PrefixedUnit::kilo(Unit::Byte), "Memory usage")
                    .context("unable to register metric memory for process probe")?,
            };
            let trigger = TriggerSpec::at_interval(config.processes.refresh_interval);
            let groups = config
                .processes
                .groups
                .into_iter()
                .map(|group| {
                    let filter = process::ProcessFilter {
                        pid: group.pid.map(|x| i32::try_from(x).unwrap_or(-1)),
                        ppid: group.ppid.map(|x| i32::try_from(x).unwrap_or(-1)),
                        exe_regex: group.exe_regex,
                    };
                    let settings = process::MonitoringSettings {
                        poll_interval: group.poll_interval,
                        flush_interval: group.flush_interval,
                    };
                    (filter, settings)
                })
                .collect();
            // We need a ScopedControlHandle to spawn new sources in the ProcessWatcher,
            // therefore it needs to be created after the pipeline startup.
            alumet.on_pipeline_start(move |ctx| {
                let control_handle = ctx.pipeline_control();
                let source = Box::new(process::ProcessWatcher::new(control_handle.clone(), metrics, groups));
                control_handle
                    .add_source("process-watcher", source, trigger)
                    .context("failed to add the process-watcher to the pipeline")
            });
        }
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct Config {
    kernel: KernelStatsMonitoring,
    memory: MeminfoMonitoring,
    processes: ProcessMonitoring,
}

#[derive(Serialize, Deserialize)]
struct KernelStatsMonitoring {
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
}

#[derive(Serialize, Deserialize)]
struct MeminfoMonitoring {
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    /// The entry to parse from /proc/meminfo.
    metrics: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct ProcessMonitoring {
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(with = "humantime_serde")]
    refresh_interval: Duration,
    groups: Vec<ProcessMonitoringGroup>,
}

#[derive(Serialize, Deserialize)]
struct ProcessMonitoringGroup {
    /// Only monitor the process that has this pid.
    pid: Option<u32>,

    /// Only monitor the processes that have this parent pid.
    ppid: Option<u32>,

    /// Only monitor the processes whose executable path matches this regex.
    #[serde(with = "serde_regex::option")]
    exe_regex: Option<Regex>,

    /// How frequently should the processes information be refreshed.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,

    /// How frequently should the processes information be flushed to the rest of the pipeline.
    #[serde(with = "humantime_serde")]
    flush_interval: Duration,
}

impl Default for KernelStatsMonitoring {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval: Duration::from_secs(5),
        }
    }
}

impl Default for MeminfoMonitoring {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval: Duration::from_secs(5),
            metrics: vec![
                "MemTotal",
                "MemFree",
                "MemAvailable",
                "Cached",
                "SwapCached",
                "Active",
                "Inactive",
                "Mapped",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }
}

impl Default for ProcessMonitoring {
    fn default() -> Self {
        Self {
            enabled: true,
            refresh_interval: Duration::from_secs(2),
            groups: vec![ProcessMonitoringGroup {
                pid: None,
                ppid: None,
                exe_regex: None, // any process; Regex::new(".*").unwrap() would also work
                poll_interval: Duration::from_secs(2),
                flush_interval: Duration::from_secs(4),
            }],
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            kernel: KernelStatsMonitoring::default(),
            memory: MeminfoMonitoring::default(),
            processes: ProcessMonitoring::default(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, Copy)]
enum CharKind {
    Lowercase,
    Uppercase,
    NonLetter,
}

impl CharKind {
    pub fn of(c: char) -> Self {
        match c {
            'a'..='z' => CharKind::Lowercase,
            'A'..='Z' => CharKind::Uppercase,
            _ => CharKind::NonLetter,
        }
    }
}

fn convert_to_snake_case(s: &str) -> String {
    assert!(s.is_ascii());
    match s.len() {
        0 => return String::new(),
        1 => return s.to_owned(),
        _ => (),
    };
    if s.contains('_') {
        return s.to_ascii_lowercase();
    }

    let mut res = String::with_capacity(s.len() + 1);
    let first = s.chars().next().unwrap();
    let mut prev = CharKind::of(first);
    res.push(first.to_ascii_lowercase());
    for ch in s.chars().skip(1) {
        let current = CharKind::of(ch);
        match (prev, current) {
            (CharKind::Lowercase, CharKind::Uppercase) | (CharKind::Uppercase, CharKind::Uppercase) => {
                res.push('_');
                res.push(ch.to_ascii_lowercase());
            }
            _ => res.push(ch.to_ascii_lowercase()),
        }
        prev = current;
    }
    res
}

#[cfg(test)]
mod tests {
    use crate::convert_to_snake_case;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_case_conversion() {
        assert_eq!("", convert_to_snake_case(""));
        assert_eq!("_", convert_to_snake_case("_"));
        assert_eq!("a", convert_to_snake_case("a"));
        assert_eq!("aa", convert_to_snake_case("aa"));
        assert_eq!("aaa", convert_to_snake_case("aaa"));
        assert_eq!("A", convert_to_snake_case("A"));
        assert_eq!("snake_case", convert_to_snake_case("snake_case"));
        assert_eq!("camel_case", convert_to_snake_case("camelCase"));
        assert_eq!("pascal_case", convert_to_snake_case("PascalCase"));
        assert_eq!("s_reclaimable", convert_to_snake_case("SReclaimable"));
        assert_eq!("nfs_unstable", convert_to_snake_case("NFS_Unstable"));
        assert_eq!("committed_as", convert_to_snake_case("Committed_AS"));
    }
}
