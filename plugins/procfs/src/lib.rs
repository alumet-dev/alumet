use alumet::{
    pipeline::{control::request, elements::source::trigger::TriggerSpec},
    plugin::{
        event,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    resources::ResourceConsumer,
    units::{PrefixedUnit, Unit},
};
use anyhow::Context;
use procfs::{Current, CurrentSI};
use rlimit::{Resource, getrlimit, setrlimit};

mod kernel;
mod memory;
mod process;
mod serde_regex;

pub struct ProcfsPlugin {
    config: Option<config::Config>,
}

impl AlumetPlugin for ProcfsPlugin {
    fn name() -> &'static str {
        "procfs"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(config::Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: config::Config = deserialize_config(config)?;
        Ok(Box::new(ProcfsPlugin { config: Some(config) }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        // TODO allow to add a source in a "paused" state for later activation?

        // Start the procfs-related sources that are enabled, according to the config.
        // Each subconfig is moved into the corresponding function, hence `config` is partially moved.
        let config = self.config.take().unwrap();
        if config.kernel.enabled {
            start_kernel_probe(config.kernel, alumet)?;
        }
        if config.memory.enabled {
            start_memory_probe(config.memory, alumet)?;
        }
        if config.processes.enabled {
            let metrics = process::ProcessMetrics {
                metric_cpu_time_delta: alumet
                    .create_metric("cpu_time_delta", PrefixedUnit::nano(Unit::Second), "CPU usage")
                    .context("unable to register metric cpu_time for process probe")?,
                metric_memory_usage: alumet
                    .create_metric("memory_usage", Unit::Byte, "Memory usage")
                    .context("unable to register metric memory for process probe")?,
            };

            match config.processes.strategy {
                config::ProcessWatchStrategy::SystemWatcher => {
                    start_process_watcher(config.processes, alumet, metrics);
                }
                config::ProcessWatchStrategy::InternalEvent => {
                    setup_process_event_listener(config.processes, alumet, metrics);
                }
            }
        }
        increase_file_descriptors_soft_limit().context("Error while increasing file descriptors soft limit")?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn start_kernel_probe(
    config_kernel: config::KernelStatsMonitoring,
    alumet: &mut alumet::plugin::AlumetPluginStart<'_>,
) -> Result<(), anyhow::Error> {
    let trigger = TriggerSpec::at_interval(config_kernel.poll_interval);
    let metrics = kernel::KernelMetrics::new(alumet).context("unable to register metrics for kernel probe")?;
    let source =
        kernel::KernelStatsProbe::new(metrics, procfs::KernelStats::PATH).context("unable to create kernel probe")?;
    alumet.add_source("kernel", Box::new(source), trigger)?;
    Ok(())
}

fn start_memory_probe(
    config_memory: config::MeminfoMonitoring,
    alumet: &mut alumet::plugin::AlumetPluginStart<'_>,
) -> Result<(), anyhow::Error> {
    let trigger = TriggerSpec::at_interval(config_memory.poll_interval);
    let metrics: anyhow::Result<Vec<_>> = config_memory
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
    let source = memory::MeminfoProbe::new(metrics?, procfs::Meminfo::PATH).context("unable to create memory probe")?;
    alumet.add_source("memory", Box::new(source), trigger)?;
    Ok(())
}

fn start_process_watcher(
    config_processes: config::ProcessMonitoring,
    alumet: &mut alumet::plugin::AlumetPluginStart<'_>,
    metrics: process::ProcessMetrics,
) {
    let trigger = TriggerSpec::at_interval(config_processes.refresh_interval);
    let groups = config_processes
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

    // We need a PluginControlHandle to spawn new sources in the ProcessWatcher,
    // therefore it needs to be created after the pipeline startup.
    //
    // Note: using `on_pipeline_start` is easier than storing a state in the plugin and
    // overriding `post_pipeline_start`.

    alumet.on_pipeline_start(move |ctx| {
        log::info!("Starting system-wide process watcher.");
        let control_handle = ctx.pipeline_control();
        let source = Box::new(process::ProcessWatcher::new(control_handle.clone(), metrics, groups));
        let create_source = request::create_one().add_source("process-watcher", source, trigger);
        ctx.block_on(control_handle.send_wait(create_source, None))
            .context("failed to add the process-watcher to the pipeline")
    });
}

fn setup_process_event_listener(
    config_processes: config::ProcessMonitoring,
    alumet: &mut alumet::plugin::AlumetPluginStart<'_>,
    metrics: process::ProcessMetrics,
) {
    let settings = process::MonitoringSettings {
        poll_interval: config_processes.events.poll_interval,
        flush_interval: config_processes.events.flush_interval,
    };

    alumet.on_pipeline_start(move |ctx| {
        log::info!("Setting up on-demand process watcher.");
        let control_handle = ctx.pipeline_control();
        let monitor = process::ManualProcessMonitor::new(control_handle.clone(), metrics, settings);
        let rt_handle = ctx.async_runtime().clone();
        event::start_consumer_measurement().subscribe(move |evt| {
            let pids = evt.0.into_iter().filter_map(|c| match c {
                ResourceConsumer::Process { pid } => Some(pid as i32),
                _ => None,
            });
            monitor.start_monitoring(pids, &rt_handle)?;
            Ok(())
        });
        Ok(())
    });
}

// prevent 'Too many open files' error
fn increase_file_descriptors_soft_limit() -> Result<(), anyhow::Error> {
    let (fd_soft, fd_hard) = getrlimit(Resource::NOFILE).context("Error while getting file descriptors limits")?;
    setrlimit(Resource::NOFILE, fd_hard, fd_hard)
        .context("Error while setting file descriptors soft limit from {fd_soft} to {fd_hard}")?;
    log::debug!(
        "Increased file descriptors soft limit ({fd_soft}) to reach hard limit value ({fd_hard}) to prevent 'Too many open files' error"
    );
    Ok(())
}

mod config {
    use std::time::Duration;

    use crate::serde_regex;
    use regex::Regex;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Default)]
    #[serde(deny_unknown_fields)]
    pub struct Config {
        pub kernel: KernelStatsMonitoring,
        pub memory: MeminfoMonitoring,
        pub processes: ProcessMonitoring,
    }

    #[derive(Serialize, Deserialize)]
    pub struct KernelStatsMonitoring {
        #[serde(default = "default_enabled")]
        pub enabled: bool,
        #[serde(with = "humantime_serde")]
        pub poll_interval: Duration,
    }

    #[derive(Serialize, Deserialize)]
    pub struct MeminfoMonitoring {
        #[serde(default = "default_enabled")]
        pub enabled: bool,
        #[serde(with = "humantime_serde")]
        pub poll_interval: Duration,
        /// The entry to parse from /proc/meminfo.
        pub metrics: Vec<String>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ProcessMonitoring {
        /// `true` to enable the monitoring of processes.
        #[serde(default = "default_enabled")]
        pub enabled: bool,

        /// Watcher refresh interval.
        #[serde(with = "humantime_serde")]
        pub refresh_interval: Duration,

        /// Groups of processes to monitor when detected.
        pub groups: Vec<ProcessMonitoringGroup>,

        /// `true` to watch for new processes, `false` to only react to Alumet events.
        #[serde(default = "default_watch_strategy")]
        pub strategy: ProcessWatchStrategy,
        pub events: EventModeProcessMonitoring,
    }

    #[derive(Serialize, Deserialize)]
    pub enum ProcessWatchStrategy {
        #[serde(rename = "watcher")]
        SystemWatcher,
        #[serde(rename = "event")]
        InternalEvent,
    }

    #[derive(Serialize, Deserialize)]
    pub struct EventModeProcessMonitoring {
        #[serde(with = "humantime_serde")]
        pub poll_interval: Duration,
        #[serde(with = "humantime_serde")]
        pub flush_interval: Duration,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ProcessMonitoringGroup {
        /// Only monitor the process that has this pid.
        pub pid: Option<u32>,

        /// Only monitor the processes that have this parent pid.
        pub ppid: Option<u32>,

        /// Only monitor the processes whose executable path matches this regex.
        #[serde(with = "serde_regex::option")]
        pub exe_regex: Option<Regex>,

        /// How frequently should the processes information be refreshed.
        #[serde(with = "humantime_serde")]
        pub poll_interval: Duration,

        /// How frequently should the processes information be flushed to the rest of the pipeline.
        #[serde(with = "humantime_serde")]
        pub flush_interval: Duration,
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
                strategy: default_watch_strategy(),
                refresh_interval: Duration::from_secs(2),
                groups: vec![ProcessMonitoringGroup {
                    pid: None,
                    ppid: None,
                    exe_regex: None, // any process; Regex::new(".*").unwrap() would also work
                    poll_interval: Duration::from_secs(2),
                    flush_interval: Duration::from_secs(4),
                }],
                events: EventModeProcessMonitoring {
                    poll_interval: Duration::from_secs(1),
                    flush_interval: Duration::from_secs(4),
                },
            }
        }
    }

    fn default_enabled() -> bool {
        true
    }

    fn default_watch_strategy() -> ProcessWatchStrategy {
        ProcessWatchStrategy::SystemWatcher
    }
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
