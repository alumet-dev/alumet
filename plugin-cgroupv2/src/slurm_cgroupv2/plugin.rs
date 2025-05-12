use alumet::{
    pipeline::{
        control::{request, PluginControlHandle},
        elements::source::trigger::TriggerSpec,
    },
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPluginStart, AlumetPostStart, ConfigTable,
    },
};
use anyhow::{anyhow, Context};
use notify::{Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

use crate::{
    cgroupv2::Metrics,
    is_accessible_dir, slurm_cgroupv2::probe::get_job_probe,
};

use super::probe::{get_all_job_probes, SlurmV2prob};

pub struct SlurmV2Plugin {
    config: SlurmV2Config,
    watcher: Option<RecommendedWatcher>,
    metrics: Option<Metrics>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]                             
struct SlurmV2Config {
    path: PathBuf,
    /// Initial interval between two cgroup measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
}

impl AlumetPlugin for SlurmV2Plugin {
    fn name() -> &'static str {
        "slurm-cgroupv2"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(SlurmV2Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config).context("invalid config")?;
        Ok(Box::new(SlurmV2Plugin {
            config,
            watcher: None,
            metrics: None,
        }))
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let v2_used = is_accessible_dir(&PathBuf::from("/sys/fs/cgroup/"))?;
        if !v2_used {
            return Err(anyhow!("Cgroups v2 are not being used!"));
        }
        self.metrics = Some(Metrics::new(alumet)?);

        let mut slurm_cgroupv2_root = PathBuf::from(&self.config.path);
        slurm_cgroupv2_root.push("system.slice/slurmstepd.scope/");
        slurm_cgroupv2_root.try_exists()?;
        let job_probes: Vec<SlurmV2prob> = get_all_job_probes(&slurm_cgroupv2_root, self.metrics.clone().unwrap())?;
        // let job_probes = Vec::<SlurmV2prob>::new();
        // let final_list_metric_file = super::utils::list_all_file(&slurm_cgroupv2_root)?;
        
        // Add as a source each job already present
        for probe in job_probes {
            alumet.add_source(
                &probe.source_name(),
                Box::new(probe),
                TriggerSpec::at_interval(self.config.poll_interval),
            ).expect("Source names should be unique (inside the plugin)");
        }

        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();

        // let metrics = self.metrics.clone().unwrap();
        let metrics = self.metrics.clone().with_context(|| "Metrics is not available")?;
        let poll_interval = self.config.poll_interval;
        let mut slurm_cgroupv2_root = PathBuf::from(&self.config.path);
        slurm_cgroupv2_root.push("system.slice/slurmstepd.scope/");
        slurm_cgroupv2_root.try_exists()?;


        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .context("tokio Runtime should build")?;

        let handler = PodDetector {
            control_handle,
            metrics,
            poll_interval,
            rt,
            slurm_cgroupv2_root : slurm_cgroupv2_root.clone(), 
        };

        let mut watcher = notify::recommended_watcher(handler)?;
        watcher.watch(&slurm_cgroupv2_root, RecursiveMode::Recursive)?;

        self.watcher = Some(watcher);

        Ok(())
    }
}

struct PodDetector {
    metrics: Metrics,
    control_handle: PluginControlHandle,
    poll_interval: Duration,
    rt: tokio::runtime::Runtime,
    slurm_cgroupv2_root: PathBuf,
}

impl EventHandler for PodDetector {
    fn handle_event(&mut self, event: Result<Event, notify::Error>) {
        fn try_handle(
            detector: &mut PodDetector,
            event: Result<Event, notify::Error>,
        ) -> Result<(), anyhow::Error> {
            // The events look like the following
            // Handle_Event: Ok(Event { kind: Create(Folder), paths: ["/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/TESTTTTT"], attr:tracker: None, attr:flag: None, attr:info: None, attr:source: None })
            // Handle_Event: Ok(Event { kind: Remove(Folder), paths: ["/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/TESTTTTT"], attr:tracker: None, attr:flag: None, attr:info: None, attr:source: None })
            if let Ok(Event {
                kind: EventKind::Create(notify::event::CreateKind::Folder),
                paths,
                ..
            }) = event
            {
                for path in paths {
                    if path.is_dir() && path.starts_with(&detector.slurm_cgroupv2_root) {
                        let job_name = path
                        .file_name()
                        .ok_or_else(|| anyhow::anyhow!("No file name found"))?
                        .to_str()
                        .context("Filename is not valid UTF-8")?
                        .to_string();
                        let probe = get_job_probe(path.clone(), detector.metrics.clone(), job_name.clone())?;
                        let source = request::create_one().add_source(
                            &probe.source_name(),
                            Box::new(probe),
                            TriggerSpec::at_interval(detector.poll_interval),
                        );
                        detector
                            .rt
                            .block_on(detector.control_handle.dispatch(source, Duration::from_secs(1)))
                            .with_context(|| format!("failed to add source for pod {job_name}"))?;
                    }
                }
                Ok(())
            } else {
                Ok(())
            }
        }

        if let Err(e) = try_handle(self, event) {
            log::error!("Error try_handle: {}", e);
        }
    }
}


impl Default for SlurmV2Config {
    fn default() -> Self {
        let root_path = PathBuf::from("/sys/fs/cgroup/");
        if !root_path.exists() {
            log::warn!("Error : Path '{}' not exist.", root_path.display());
        }
        Self {
            path: root_path,
            poll_interval: Duration::from_secs(1), // 1Hz
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cgroupv2::tests_mock::{CpuStatMock, MemoryCurrentMock, MemoryStatMock, MockFileCgroupKV};

    use super::*;
    use alumet::{
        agent::{
            self,
            plugin::{PluginInfo, PluginSet},
        }, measurement::WrappedMeasurementValue, pipeline::naming::SourceName, plugin::PluginMetadata, resources::ResourceConsumer, test::RuntimeExpectations, units::{PrefixedUnit, Unit}
    };
    
    use alumet::test::StartupExpectations;
    use anyhow::Result;
    use tempfile::tempdir;
    use std::{fs::{self, File, OpenOptions}, path::PathBuf};

    const TIMEOUT: Duration = Duration::from_secs(5);

    // Create a fake plugin structure for slurm_cgroupv2 plugin
    fn create_mock_plugin() -> SlurmV2Plugin {
        SlurmV2Plugin {
            config: SlurmV2Config {
                path: PathBuf::from("/sys/fs/cgroup/"), //TODO change ?
                poll_interval: Duration::from_secs(1),
            },
            watcher: None,
            metrics: None,
        }
    }

    // Test `default_config` function of slurm_cgroupv2 plugin
    #[test]
    fn test_default_config() {
        let result = SlurmV2Plugin::default_config().unwrap();
        assert!(result.is_some(), "result : None");

        let config_table = result.unwrap();
        let config: SlurmV2Config = deserialize_config(config_table).expect("ERROR : Failed to deserialize config");

        assert_eq!(config.path, PathBuf::from("/sys/fs/cgroup/"));
        assert_eq!(config.poll_interval, Duration::from_secs(1));
    }

    // Test `init` function to initialize slurm_cgroupv2 plugin configuration
    #[test]
    fn test_init() -> Result<()> {
        let config_table = serialize_config(SlurmV2Config::default())?;
        let plugin = SlurmV2Plugin::init(config_table)?;
        assert!(plugin.metrics.is_none());
        assert!(plugin.watcher.is_none());
        Ok(())
    }

    // Test `stop` function to stop slurm_cgroupv2 plugin
    #[test]
    fn test_stop() {
        let mut plugin = create_mock_plugin();
        let result = plugin.stop();
        assert!(result.is_ok(), "Stop should complete without errors.");
    }

    #[test]
    fn test_correct_run_with_no_jobs() {
        // Creation of file hierarchy
        let root = tempdir().unwrap().into_path();
        let _result = create_cgroupv2_tree_slurm_empty(&root);
        
        let mut plugins = PluginSet::new();
        let config = SlurmV2Config {
            path: root,
            poll_interval: Duration::from_secs(1),
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<SlurmV2Plugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&config)),
        });

        let startup_expectations = StartupExpectations::new()
        .expect_metric::<u64>("cgroup_memory_anonymous", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_file", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_kernel_stack", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_pagetables", Unit::Byte.clone())
        .expect_metric::<u64>("memory_usage", Unit::Byte.clone())
        .expect_metric::<u64>("cpu_time_delta", PrefixedUnit::nano(Unit::Second))//;
        .expect_source("slurm-cgroupv2", "job:slurmstepd.scope")
        .expect_source("slurm-cgroupv2", "job:system");

        let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectations)
        .build_and_start()
        .unwrap();

        // Send shutdown message
        agent.pipeline.control_handle().shutdown();
        agent.wait_for_shutdown(TIMEOUT).unwrap();

        return
    }

    #[test]
    fn test_correct_run_with_one_job() {
        // Creation of file hierarchy
        let root = tempdir().unwrap().into_path();
        let _result = create_cgroupv2_tree_slurm_job(&root);
        
        let mut plugins = PluginSet::new();
        let config = SlurmV2Config {
            path: root.clone(),
            poll_interval: Duration::from_secs(1),
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<SlurmV2Plugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&config)),
        });

        let startup_expectations = StartupExpectations::new()
        .expect_metric::<u64>("cgroup_memory_anonymous", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_file", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_kernel_stack", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_pagetables", Unit::Byte.clone())
        .expect_metric::<u64>("memory_usage", Unit::Byte.clone())
        .expect_metric::<u64>("cpu_time_delta", PrefixedUnit::nano(Unit::Second));

        let pah_src1 = root.clone();
        let pah_src2 = root.clone();
        let run_expect = RuntimeExpectations::new().test_source(
            SourceName::from_str("slurm-cgroupv2", "job:job_12345"), move || {
                let mut mock = CpuStatMock::default();
                mock.usage_usec = 50;
                mock.user_usec = 10;
                mock.system_usec = 75;
                let mut path_to_cpu_stat = PathBuf::from(&pah_src1.clone());
                path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_12345/cpu.stat");
                let file = OpenOptions::new()
                .read(true)   // Allow read
                .write(true)  // Allows write
                .open(path_to_cpu_stat.clone())
                .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                mock.replace_to_file(file).unwrap();
                
            }, |_m| {}).test_source(
                SourceName::from_str("slurm-cgroupv2", "job:job_12345"), move || {
                    let mut mock = CpuStatMock::default();
                    mock.usage_usec = 55; // +5
                    mock.user_usec = 12; // +2
                    mock.system_usec = 78; // +3
                    let mut path_to_cpu_stat = PathBuf::from(&pah_src2);
                    path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_12345/cpu.stat");
                    let file = OpenOptions::new()
                    .read(true)   // Allow read
                    .write(true)  // Allows write
                    .open(path_to_cpu_stat.clone())
                    .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                    mock.replace_to_file(file).unwrap();
                    
                }, |m| {
                    assert_eq!(m.len(), 7);
                    for elm in m {
                        assert!(elm.attributes_keys().any(|k| k == "job_name"));
                        if let ResourceConsumer::ControlGroup { path } = &elm.consumer {
                            if path.contains("cpu.stat"){
                                assert!( elm.value == WrappedMeasurementValue::U64(2) || elm.value == WrappedMeasurementValue::U64(3));
                            }
                        };
                    }
                });

        let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectations)
        .with_expectations(run_expect)
        .build_and_start()
        .unwrap();

        // Send shutdown message
        agent.wait_for_shutdown(TIMEOUT).unwrap();

        return
    }

    #[test]
    fn test_correct_run_with_two_jobs() {
        // Creation of file hierarchy
        let root = tempdir().unwrap().into_path();
        let _result = create_cgroupv2_tree_slurm_jobs(&root);
        
        let mut plugins = PluginSet::new();
        let config = SlurmV2Config {
            path: root.clone(),
            poll_interval: Duration::from_secs(1),
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<SlurmV2Plugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&config)),
        });

        let startup_expectations = StartupExpectations::new()
        .expect_metric::<u64>("cgroup_memory_anonymous", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_file", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_kernel_stack", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_pagetables", Unit::Byte.clone())
        .expect_metric::<u64>("memory_usage", Unit::Byte.clone())
        .expect_metric::<u64>("cpu_time_delta", PrefixedUnit::nano(Unit::Second));

        let path_src11 = root.clone();
        let path_src12 = root.clone();
        let path_src21 = root.clone();
        let path_src22 = root.clone();

        let run_expect = RuntimeExpectations::new().test_source(
            SourceName::from_str("slurm-cgroupv2", "job:job_12345"), move || {
                let mut mock = CpuStatMock::default();
                mock.usage_usec = 100;
                mock.user_usec = 110;
                mock.system_usec = 120;
                let mut path_to_cpu_stat = PathBuf::from(&path_src11.clone());
                path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_12345/cpu.stat");
                let file = OpenOptions::new()
                .read(true)   // Allow read
                .write(true)  // Allows write
                .open(path_to_cpu_stat.clone())
                .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                mock.replace_to_file(file).unwrap();
                
            }, |_m| {}).test_source(
                SourceName::from_str("slurm-cgroupv2", "job:job_12345"), move || {
                    let mut mock = CpuStatMock::default();
                    mock.usage_usec = 115; // +15
                    mock.user_usec = 120; // +10
                    mock.system_usec = 125; // +5
                    let mut path_to_cpu_stat = PathBuf::from(&path_src12);
                    path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_12345/cpu.stat");
                    let file = OpenOptions::new()
                    .read(true)   // Allow read
                    .write(true)  // Allows write
                    .open(path_to_cpu_stat.clone())
                    .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                    mock.replace_to_file(file).unwrap();
                    
                }, |m| {
                    assert_eq!(m.len(), 7);
                    for elm in m {
                        assert!(elm.attributes_keys().any(|k| k == "job_name"));
                        if let ResourceConsumer::ControlGroup { path } = &elm.consumer {
                            if path.contains("cpu.stat"){
                                assert!( elm.value == WrappedMeasurementValue::U64(10) || elm.value == WrappedMeasurementValue::U64(5));
                            }
                        };
                    }
                }).test_source(
            SourceName::from_str("slurm-cgroupv2", "job:job_67890"), move || {
                let mut mock = CpuStatMock::default();
                mock.usage_usec = 200;
                mock.user_usec = 210;
                mock.system_usec = 220;
                let mut path_to_cpu_stat = PathBuf::from(&path_src21.clone());
                path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_67890/cpu.stat");
                let file = OpenOptions::new()
                .read(true)   // Allow read
                .write(true)  // Allows write
                .open(path_to_cpu_stat.clone())
                .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                mock.replace_to_file(file).unwrap();
                
            }, |_m| {}).test_source(
                SourceName::from_str("slurm-cgroupv2", "job:job_67890"), move || {
                    let mut mock = CpuStatMock::default();
                    mock.usage_usec = 230; // +30
                    mock.user_usec = 245; // +35
                    mock.system_usec = 240; // +20
                    let mut path_to_cpu_stat = PathBuf::from(&path_src22);
                    path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_67890/cpu.stat");
                    let file = OpenOptions::new()
                    .read(true)   // Allow read
                    .write(true)  // Allows write
                    .open(path_to_cpu_stat.clone())
                    .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                    mock.replace_to_file(file).unwrap();
                    
                }, |m| {
                    assert_eq!(m.len(), 7);
                    for elm in m {
                        assert!(elm.attributes_keys().any(|k| k == "job_name"));
                        if let ResourceConsumer::ControlGroup { path } = &elm.consumer {
                            if path.contains("cpu.stat"){
                                assert!( elm.value == WrappedMeasurementValue::U64(35) || elm.value == WrappedMeasurementValue::U64(20));
                            }
                        };
                    }
                });

        let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectations)
        .with_expectations(run_expect)
        .build_and_start()
        .unwrap();

        // Send shutdown message
        // agent.pipeline.control_handle().shutdown();
        // agent.wait_for_shutdown(TIMEOUT).unwrap();
        agent.wait_for_shutdown(Duration::from_secs(10)).unwrap();

        return
    }

    #[test]
    fn test_correct_run_with_one_job_coming_later() {
        // Creation of file hierarchy
        let root = tempdir().unwrap().into_path();
        let _result = create_cgroupv2_tree_slurm_empty(&root);
        
        let mut plugins = PluginSet::new();
        let config = SlurmV2Config {
            path: root.clone(),
            poll_interval: Duration::from_secs(1),
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<SlurmV2Plugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&config)),
        });

        let startup_expectations = StartupExpectations::new()
        .expect_metric::<u64>("cgroup_memory_anonymous", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_file", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_kernel_stack", Unit::Byte.clone())
        .expect_metric::<u64>("cgroup_memory_pagetables", Unit::Byte.clone())
        .expect_metric::<u64>("memory_usage", Unit::Byte.clone())
        .expect_metric::<u64>("cpu_time_delta", PrefixedUnit::nano(Unit::Second));

        let path_slurmstepd = root.clone();
        let path_job = root.clone();
        let run_expect = RuntimeExpectations::new().test_source(
            SourceName::from_str("slurm-cgroupv2", "job:slurmstepd.scope"), move || {              
            }, |_m| {}).test_source(
            SourceName::from_str("slurm-cgroupv2", "job:job_12345"), move || {
                let mut mock = CpuStatMock::default();
                mock.usage_usec = 100;
                mock.user_usec = 110;
                mock.system_usec = 120;
                let mut path_to_cpu_stat = PathBuf::from(&path_job.clone());
                path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_12345/cpu.stat");
                let file = OpenOptions::new()
                .read(true)   // Allow read
                .write(true)  // Allows write
                .open(path_to_cpu_stat.clone())
                .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                mock.replace_to_file(file).unwrap();
                
            }, |_m| {});

        let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectations)
        .with_expectations(run_expect)
        .build_and_start()
        .unwrap();

        create_cgroupv2_tree_slurm_job(&path_slurmstepd.clone()).expect("Should be able to create a new cgeoupv2 tree");

        // Send shutdown message
        agent.wait_for_shutdown(TIMEOUT).unwrap();

        return
    }


    fn config_to_toml_table(config: &SlurmV2Config) -> toml::Table {
        toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
    }

    fn create_files(root: &PathBuf) -> Result<(), anyhow::Error> {
        let cpu_stat_mock_file = CpuStatMock::default();
        let mem_stat_mock_file: MemoryStatMock = MemoryStatMock::default();
        let mem_current_mock_file: MemoryCurrentMock = MemoryCurrentMock(0);

        let file_path_cpu = root.join("cpu.stat");
        let file_cpu = File::create(file_path_cpu.clone())?;
        cpu_stat_mock_file.write_to_file(file_cpu)?;
        assert!(file_path_cpu.exists());

        let file_path_mem_stat = root.join("memory.stat");
        let file_mem_stat = File::create(file_path_mem_stat.clone())?;
        mem_stat_mock_file.write_to_file(file_mem_stat)?;
        assert!(file_path_mem_stat.exists());

        let file_path_mem_current = root.join("memory.current");
        let file_mem_current = File::create(file_path_mem_current.clone())?;
        mem_current_mock_file.write_to_file(file_mem_current)?;
        assert!(file_path_mem_current.exists());

        Ok(())
    }

    fn create_folder(root: &PathBuf, name: &str) -> Result<(), anyhow::Error> {
        let dir_path = root.join(name);
        fs::create_dir_all(&dir_path).unwrap();
        assert!(is_accessible_dir(&dir_path).unwrap());
        create_files(&dir_path)?;
        Ok(())

    }

    fn create_cgroupv2_tree_slurm_empty(root: &PathBuf) -> Result<(), anyhow::Error>  {
        create_files(&root)?;
        create_folder(&root, "system.slice")?;
        let path_to_system = root.clone().join("system.slice/");
        
        create_folder(&path_to_system, "slurmstepd.scope")?;
        let path_to_slurmstepd = path_to_system.clone().join("slurmstepd.scope/");
               
        create_files(&path_to_slurmstepd)?;
        create_folder(&path_to_slurmstepd, "system")?;
        Ok(())
    }

    fn create_cgroupv2_tree_slurm_job(root: &PathBuf) -> Result<(), anyhow::Error>  {
        let job_name = "job_12345";
        create_files(&root)?;
        create_folder(&root, "system.slice")?;
        let path_to_system = root.clone().join("system.slice/");
        
        create_folder(&path_to_system, "slurmstepd.scope")?;
        let path_to_slurmstepd = path_to_system.clone().join("slurmstepd.scope/");
        
        // create_folder(&path_to_slurmstepd, "step_extern")?;

        create_folder(&path_to_slurmstepd, job_name)?;
        let path_inside_job = path_to_slurmstepd.clone().join(job_name);
        create_folder(&path_inside_job, "step_batch")?;
        create_folder(&path_inside_job, "step_extern")?;
        Ok(())
    }

    fn create_cgroupv2_tree_slurm_jobs(root: &PathBuf) -> Result<(), anyhow::Error>  {
        let job_name1 = "job_12345";
        let job_name2 = "job_67890";
        create_files(&root)?;
        create_folder(&root, "system.slice")?;
        let path_to_system = root.clone().join("system.slice/");
        
        create_folder(&path_to_system, "slurmstepd.scope")?;
        let path_to_slurmstepd = path_to_system.clone().join("slurmstepd.scope/");
        
        // create_folder(&path_to_slurmstepd, "step_extern")?;

        create_folder(&path_to_slurmstepd, job_name1)?;
        let path_inside_job1 = path_to_slurmstepd.clone().join(job_name1);
        create_folder(&path_inside_job1, "step_batch")?;
        create_folder(&path_inside_job1, "step_extern")?;

        create_folder(&path_to_slurmstepd, job_name2)?;
        let path_inside_job2 = path_to_slurmstepd.clone().join(job_name2);
        create_folder(&path_inside_job2, "step_batch")?;
        create_folder(&path_inside_job2, "step_extern")?;
        Ok(())
    }
}
