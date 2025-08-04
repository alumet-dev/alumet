#[cfg(test)]
mod tests {
    use alumet::{
        agent::{
            self,
            plugin::{PluginInfo, PluginSet},
        },
        measurement::WrappedMeasurementValue,
        pipeline::naming::SourceName,
        plugin::{
            rust::{deserialize_config, serialize_config, AlumetPlugin},
            PluginMetadata,
        },
        resources::ResourceConsumer,
        test::RuntimeExpectations,
        units::{PrefixedUnit, Unit},
    };
    use plugin_cgroup::plugins::{slurm::Config, SlurmPlugin};

    use alumet::test::StartupExpectations;
    use anyhow::Result;
    use std::{
        fs::{self, File, OpenOptions},
        path::{Path, PathBuf},
        time::Duration,
    };
    use tempfile::tempdir;
    use util_cgroups::measure::v2::mock::{CpuStatMock, MemoryStatMock, MockFileCgroupKV};

    const TIMEOUT: Duration = Duration::from_secs(5);

    // Create a fake plugin structure for slurm_cgroupv2 plugin
    fn create_mock_plugin() -> SlurmPlugin {
        SlurmPlugin {
            config: Some(Config {
                poll_interval: Duration::from_secs(1),
                ..Default::default()
            }),
            starting_state: None,
            reactor: None,
        }
    }

    // Test `default_config` function of slurm_cgroupv2 plugin
    #[test]
    fn test_default_config() {
        let result = SlurmPlugin::default_config().unwrap();
        assert!(result.is_some(), "result : None");

        let config_table = result.unwrap();
        let config: Config = deserialize_config(config_table).expect("ERROR : Failed to deserialize config");

        assert_eq!(config.jobs_only, true);
        assert_eq!(config.poll_interval, Duration::from_secs(1));
    }

    // Test `init` function to initialize slurm_cgroupv2 plugin configuration
    #[test]
    fn test_init() -> Result<()> {
        let config_table = serialize_config(Config::default())?;
        let plugin = SlurmPlugin::init(config_table)?;
        assert!(plugin.reactor.is_none());
        assert!(plugin.starting_state.is_none());
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
        let root = tempdir().unwrap().path().to_path_buf();
        let _result = create_cgroupv2_tree_slurm_empty(&root);

        let mut plugins = PluginSet::new();
        let config = Config {
            poll_interval: Duration::from_secs(1),
            jobs_only: true,
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<SlurmPlugin>(),
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

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup_expectations)
            .build_and_start()
            .unwrap();

        // Send shutdown message
        agent.pipeline.control_handle().shutdown();
        agent.wait_for_shutdown(TIMEOUT).unwrap();

        return;
    }

    #[test]
    fn test_correct_run_with_one_job() {
        // Creation of file hierarchy
        let root = tempdir().unwrap().path().to_path_buf();
        let _result = create_cgroupv2_tree_slurm_job(&root);

        let mut plugins = PluginSet::new();
        let config = Config {
            poll_interval: Duration::from_secs(1),
            ..Default::default()
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<SlurmPlugin>(),
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
        let run_expect = RuntimeExpectations::new()
            .test_source(
                SourceName::from_str("slurm-cgroupv2", "job:job_12345"),
                move || {
                    let mut mock = CpuStatMock::default();
                    mock.usage_usec = 50;
                    mock.user_usec = 10;
                    mock.system_usec = 75;
                    let mut path_to_cpu_stat = PathBuf::from(&pah_src1.clone());
                    path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_12345/cpu.stat");
                    let mut file = OpenOptions::new()
                        .read(true) // Allow read
                        .write(true) // Allows write
                        .open(path_to_cpu_stat.clone())
                        .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                    mock.write_to_file(&mut file).unwrap();
                },
                |_m| {},
            )
            .test_source(
                SourceName::from_str("slurm-cgroupv2", "job:job_12345"),
                move || {
                    let mut mock = CpuStatMock::default();
                    mock.usage_usec = 55; // +5
                    mock.user_usec = 12; // +2
                    mock.system_usec = 78; // +3
                    let mut path_to_cpu_stat = PathBuf::from(&pah_src2);
                    path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_12345/cpu.stat");
                    let mut file = OpenOptions::new()
                        .read(true) // Allow read
                        .write(true) // Allows write
                        .open(path_to_cpu_stat.clone())
                        .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                    mock.write_to_file(&mut file).unwrap();
                },
                |m| {
                    assert_eq!(m.len(), 7);
                    for elm in m {
                        assert!(elm.attributes_keys().any(|k| k == "job_name"));
                        if let ResourceConsumer::ControlGroup { path } = &elm.consumer {
                            if path.contains("cpu.stat") {
                                assert!(
                                    elm.value == WrappedMeasurementValue::U64(2)
                                        || elm.value == WrappedMeasurementValue::U64(3)
                                );
                            }
                        };
                    }
                },
            );

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup_expectations)
            .with_expectations(run_expect)
            .build_and_start()
            .unwrap();

        // Send shutdown message
        agent.wait_for_shutdown(TIMEOUT).unwrap();

        return;
    }

    #[test]
    fn test_correct_run_with_two_jobs() {
        // Creation of file hierarchy
        let root = tempdir().unwrap().path().to_path_buf();
        let _result = create_cgroupv2_tree_slurm_jobs(&root);

        let mut plugins = PluginSet::new();
        let config = Config {
            poll_interval: Duration::from_secs(1),
            ..Default::default()
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<SlurmPlugin>(),
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

        let run_expect = RuntimeExpectations::new()
            .test_source(
                SourceName::from_str("slurm-cgroupv2", "job:job_12345"),
                move || {
                    let mut mock = CpuStatMock::default();
                    mock.usage_usec = 100;
                    mock.user_usec = 110;
                    mock.system_usec = 120;
                    let mut path_to_cpu_stat = PathBuf::from(&path_src11.clone());
                    path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_12345/cpu.stat");
                    let mut file = OpenOptions::new()
                        .read(true) // Allow read
                        .write(true) // Allows write
                        .open(path_to_cpu_stat.clone())
                        .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                    mock.write_to_file(&mut file).unwrap();
                },
                |_m| {},
            )
            .test_source(
                SourceName::from_str("slurm-cgroupv2", "job:job_12345"),
                move || {
                    let mut mock = CpuStatMock::default();
                    mock.usage_usec = 115; // +15
                    mock.user_usec = 120; // +10
                    mock.system_usec = 125; // +5
                    let mut path_to_cpu_stat = PathBuf::from(&path_src12);
                    path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_12345/cpu.stat");
                    let mut file = OpenOptions::new()
                        .read(true) // Allow read
                        .write(true) // Allows write
                        .open(path_to_cpu_stat.clone())
                        .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                    mock.write_to_file(&mut file).unwrap();
                },
                |m| {
                    assert_eq!(m.len(), 7);
                    for elm in m {
                        assert!(elm.attributes_keys().any(|k| k == "job_name"));
                        if let ResourceConsumer::ControlGroup { path } = &elm.consumer {
                            if path.contains("cpu.stat") {
                                assert!(
                                    elm.value == WrappedMeasurementValue::U64(10)
                                        || elm.value == WrappedMeasurementValue::U64(5)
                                );
                            }
                        };
                    }
                },
            )
            .test_source(
                SourceName::from_str("slurm-cgroupv2", "job:job_67890"),
                move || {
                    let mut mock = CpuStatMock::default();
                    mock.usage_usec = 200;
                    mock.user_usec = 210;
                    mock.system_usec = 220;
                    let mut path_to_cpu_stat = PathBuf::from(&path_src21.clone());
                    path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_67890/cpu.stat");
                    let mut file = OpenOptions::new()
                        .read(true) // Allow read
                        .write(true) // Allows write
                        .open(path_to_cpu_stat.clone())
                        .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                    mock.write_to_file(&mut file).unwrap();
                },
                |_m| {},
            )
            .test_source(
                SourceName::from_str("slurm-cgroupv2", "job:job_67890"),
                move || {
                    let mut mock = CpuStatMock::default();
                    mock.usage_usec = 230; // +30
                    mock.user_usec = 245; // +35
                    mock.system_usec = 240; // +20
                    let mut path_to_cpu_stat = PathBuf::from(&path_src22);
                    path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_67890/cpu.stat");
                    let mut file = OpenOptions::new()
                        .read(true) // Allow read
                        .write(true) // Allows write
                        .open(path_to_cpu_stat.clone())
                        .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                    mock.write_to_file(&mut file).unwrap();
                },
                |m| {
                    assert_eq!(m.len(), 7);
                    for elm in m {
                        assert!(elm.attributes_keys().any(|k| k == "job_name"));
                        if let ResourceConsumer::ControlGroup { path } = &elm.consumer {
                            if path.contains("cpu.stat") {
                                assert!(
                                    elm.value == WrappedMeasurementValue::U64(35)
                                        || elm.value == WrappedMeasurementValue::U64(20)
                                );
                            }
                        };
                    }
                },
            );

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup_expectations)
            .with_expectations(run_expect)
            .build_and_start()
            .unwrap();

        // Send shutdown message
        // agent.pipeline.control_handle().shutdown();
        // agent.wait_for_shutdown(TIMEOUT).unwrap();
        agent.wait_for_shutdown(Duration::from_secs(10)).unwrap();

        return;
    }

    #[test]
    fn test_correct_run_with_one_job_coming_later() {
        // Creation of file hierarchy
        // let root = tempdir().unwrap().path().to_path_buf();

        let root_b = tempdir().unwrap();
        let root = root_b.path().to_path_buf();
        let _result = create_cgroupv2_tree_slurm_empty(&root);

        let mut plugins = PluginSet::new();
        let config = Config {
            poll_interval: Duration::from_secs(1),
            ..Default::default()
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<SlurmPlugin>(),
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
        let run_expect = RuntimeExpectations::new()
            .test_source(
                SourceName::from_str("slurm-cgroupv2", "job:slurmstepd.scope"),
                move || {},
                |_m| {},
            )
            .test_source(
                SourceName::from_str("slurm-cgroupv2", "job:job_12345"),
                move || {
                    let mut mock = CpuStatMock::default();
                    mock.usage_usec = 100;
                    mock.user_usec = 110;
                    mock.system_usec = 120;
                    let mut path_to_cpu_stat = PathBuf::from(&path_job.clone());
                    path_to_cpu_stat = path_to_cpu_stat.join("system.slice/slurmstepd.scope/job_12345/cpu.stat");
                    let mut file = OpenOptions::new()
                        .read(true) // Allow read
                        .write(true) // Allows write
                        .open(path_to_cpu_stat.clone())
                        .expect("Error when trying to open cpu.stat of slurmstep.scope folder");
                    mock.write_to_file(&mut file).unwrap();
                },
                |_m| {},
            );

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup_expectations)
            .with_expectations(run_expect)
            .build_and_start()
            .unwrap();

        create_cgroupv2_tree_slurm_job(&path_slurmstepd.clone()).expect("Cannot create a new cgroupv2 tree");

        // Send shutdown message
        agent.wait_for_shutdown(TIMEOUT).unwrap();

        return;
    }

    fn config_to_toml_table(config: &Config) -> toml::Table {
        toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
    }

    /// Check if a specific file is a dir. Used to know if cgroup v2 are used.
    ///
    /// # Return value
    ///
    /// Returns `Ok(true)` if it can be verified that `path` is a directory, and `Ok(false)` if it can be verified that it is not a directory.
    /// Returns an error if the path metadata cannot be obtained.
    pub fn is_accessible_dir(path: &Path) -> Result<bool, std::io::Error> {
        match std::fs::metadata(path) {
            Ok(metadata) => Ok(metadata.is_dir()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn create_files(root: &PathBuf) -> Result<(), anyhow::Error> {
        let cpu_stat_mock_file = CpuStatMock::default();
        let mem_stat_mock_file: MemoryStatMock = MemoryStatMock::default();

        let file_path_cpu = root.join("cpu.stat");
        let mut file_cpu = File::create(file_path_cpu.clone())?;
        cpu_stat_mock_file.write_to_file(&mut file_cpu)?;
        assert!(file_path_cpu.exists());

        let file_path_mem_stat = root.join("memory.stat");
        let mut file_mem_stat = File::create(file_path_mem_stat.clone())?;
        mem_stat_mock_file.write_to_file(&mut file_mem_stat)?;
        assert!(file_path_mem_stat.exists());

        Ok(())
    }

    fn create_folder(root: &PathBuf, name: &str) -> Result<(), anyhow::Error> {
        let dir_path = root.join(name);
        fs::create_dir_all(&dir_path).unwrap();
        assert!(is_accessible_dir(&dir_path).unwrap());
        create_files(&dir_path)?;
        Ok(())
    }

    fn create_cgroupv2_tree_slurm_empty(root: &PathBuf) -> Result<(), anyhow::Error> {
        create_files(&root)?;
        create_folder(&root, "system.slice")?;
        let path_to_system = root.clone().join("system.slice/");

        create_folder(&path_to_system, "slurmstepd.scope")?;
        let path_to_slurmstepd = path_to_system.clone().join("slurmstepd.scope/");

        create_files(&path_to_slurmstepd)?;
        Ok(())
    }

    fn create_cgroupv2_tree_slurm_job(root: &PathBuf) -> Result<(), anyhow::Error> {
        let job_name = "job_12345";
        create_files(&root)?;
        create_folder(&root, "system.slice")?;
        let path_to_system = root.clone().join("system.slice/");

        create_folder(&path_to_system, "slurmstepd.scope")?;
        let path_to_slurmstepd = path_to_system.clone().join("slurmstepd.scope/");

        // create_folder(&path_to_slurmstepd, "step_extern")?;

        create_folder(&path_to_slurmstepd, job_name)?;
        let path_inside_job = path_to_slurmstepd.clone().join(job_name);
        create_folder(&path_inside_job, "step_0")?;
        Ok(())
    }

    fn create_cgroupv2_tree_slurm_jobs(root: &PathBuf) -> Result<(), anyhow::Error> {
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
        create_folder(&path_inside_job1, "step_0")?;

        create_folder(&path_to_slurmstepd, job_name2)?;
        let path_inside_job2 = path_to_slurmstepd.clone().join(job_name2);
        create_folder(&path_inside_job2, "step_0")?;
        Ok(())
    }
}
