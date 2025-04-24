#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        io::{self, Write},
        path::Path,
        thread, time,
        time::Duration,
    };

    use alumet::{
        agent::{
            self,
            plugin::{PluginInfo, PluginSet},
        },
        measurement::{AttributeValue, WrappedMeasurementValue},
        pipeline::naming::SourceName,
        plugin::PluginMetadata,
        test::{RuntimeExpectations, StartupExpectations},
        units::{PrefixedUnit, Unit},
    };
    use plugin_cgroupv1::{Oar2Config, Oar2Plugin};
    use tempfile::tempdir;

    /// This test ensure the plugin startup correctly, with the expected source based on OAR Mocks created during the test.
    /// It also verifies the registered metrics and their units.
    #[test]
    fn test_startup() -> Result<(), Box<dyn std::error::Error>> {
        let mut plugins = PluginSet::new();
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path();

        let mocks = vec![
            OarMock {
                submitter: "somesubmitter",
                job_id: "1234",
                cgroup_files: vec![
                    CgroupFile::CpuAcctUsage(123456789),
                    CgroupFile::MemoryUsageInBytes(987654321),
                ],
            },
            OarMock {
                submitter: "somesubmitter",
                job_id: "5678",
                cgroup_files: vec![
                    CgroupFile::CpuAcctUsage(123456789),
                    CgroupFile::MemoryUsageInBytes(987654321),
                ],
            },
            OarMock {
                submitter: "anothersubmitter",
                job_id: "666",
                cgroup_files: vec![
                    CgroupFile::CpuAcctUsage(123456789),
                    CgroupFile::MemoryUsageInBytes(987654321),
                ],
            },
        ];
        create_oar_mocks(path, mocks)?;

        let source_config = Oar2Config {
            path: path.to_path_buf(),
            poll_interval: Duration::from_secs(1),
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<Oar2Plugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&source_config)),
        });

        let startup_expectations = StartupExpectations::new()
            .expect_metric::<u64>("cpu_time_delta", PrefixedUnit::nano(Unit::Second))
            .expect_metric::<u64>("memory_usage", Unit::Byte)
            .expect_source("oar2-plugin", "somesubmitter_1234")
            .expect_source("oar2-plugin", "somesubmitter_5678")
            .expect_source("oar2-plugin", "anothersubmitter_666");

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup_expectations)
            .build_and_start()
            .unwrap();

        agent.pipeline.control_handle().shutdown();
        agent.wait_for_shutdown(Duration::from_secs(10)).unwrap();

        Ok(())
    }

    /// This test performs lot of different tests in order to verify the plugin run correctly at runtime:
    /// It's orchestrated in a way to test different scenario: first poll and other polls, since first poll will not have measurements for CounterDiff metrics.
    /// It also ensure cgroup that pop up during Alumet's runtime are correctly collected too.
    #[test]
    fn test_runtime() -> anyhow::Result<()> {
        let mut plugins = PluginSet::new();
        let temp_dir = tempdir()?;
        let path = temp_dir.path().to_path_buf();

        let mocks = vec![
            OarMock {
                submitter: "somesubmitter",
                job_id: "1234",
                cgroup_files: vec![
                    CgroupFile::CpuAcctUsage(123456789),
                    CgroupFile::MemoryUsageInBytes(987654321),
                ],
            },
            OarMock {
                submitter: "somesubmitter",
                job_id: "5678",
                cgroup_files: vec![
                    CgroupFile::CpuAcctUsage(987654321),
                    CgroupFile::MemoryUsageInBytes(123456789),
                ],
            },
        ];
        create_oar_mocks(&path, mocks)?;

        let source_config = Oar2Config {
            path: path.to_path_buf(),
            poll_interval: Duration::from_secs(1),
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<Oar2Plugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&source_config)),
        });

        let runtime_expectations = RuntimeExpectations::new()
            .test_source(
                SourceName::from_str("oar2-plugin", "somesubmitter_1234"),
                || (),
                |m| {
                    //note: it's expected to have only memory_usage measurement as at first call of poll, cpu_time_delta is not initialized yet
                    assert_eq!(m.len(), 1);
                    let memory_usage = m.iter().next().unwrap();
                    assert_eq!(memory_usage.value, WrappedMeasurementValue::U64(987654321));
                },
            )
            .test_source(SourceName::from_str("oar2-plugin", "somesubmitter_5678"), || (), {
                let path = path.clone();
                move |m| {
                    //note: it's expected to have only memory_usage measurement as at first call of poll, cpu_time_delta is not initialized yet
                    assert_eq!(m.len(), 1);
                    let memory_usage = m.iter().next().unwrap();
                    let attributes: Vec<_> = memory_usage.attributes().collect();
                    assert_eq!(attributes.len(), 2);
                    let kind_attribute = attributes[0];
                    let job_id_attribute = attributes[1];
                    assert_eq!(kind_attribute.0, "kind");
                    assert_eq!(job_id_attribute.0, "job_id");
                    if let AttributeValue::Str(kind) = kind_attribute.1 {
                        assert_eq!(*kind, "resident");
                    } else {
                        assert!(false, "kind attribute should be of str type");
                    }
                    if let AttributeValue::String(job_id) = job_id_attribute.1 {
                        assert_eq!(job_id, "5678");
                    } else {
                        assert!(false, "job_id attribute should be of string type");
                    }
                    assert_eq!(memory_usage.value, WrappedMeasurementValue::U64(123456789));

                    // creating new mocks to make values change for next poll and also verify Event/Notification mechanism to manage new cgroup at runtime
                    let mocks = vec![
                        OarMock {
                            submitter: "somesubmitter",
                            job_id: "5678",
                            cgroup_files: vec![
                                CgroupFile::CpuAcctUsage(987654331),
                                CgroupFile::MemoryUsageInBytes(987654321),
                            ],
                        },
                        OarMock {
                            submitter: "anewsubmitter",
                            job_id: "666",
                            cgroup_files: vec![
                                CgroupFile::CpuAcctUsage(123123123),
                                CgroupFile::MemoryUsageInBytes(321321321),
                            ],
                        },
                    ];
                    create_oar_mocks(&path, mocks).expect("error creating oar mock");
                    let memory_usage_consumer_path = format!("{}", memory_usage.consumer.id_display());
                    assert_eq!(
                        memory_usage_consumer_path,
                        format!(
                            "{0}/memory/oar/somesubmitter_5678/memory.usage_in_bytes",
                            path.to_str().unwrap()
                        )
                    );
                }
            })
            .test_source(
                SourceName::from_str("oar2-plugin", "somesubmitter_5678"),
                || (),
                move |m| {
                    assert_eq!(m.len(), 2);
                    let mut measurements = m.iter();
                    let cpu_time_delta = measurements.next().unwrap();
                    let attributes: Vec<_> = cpu_time_delta.attributes().collect();
                    assert_eq!(attributes.len(), 2);
                    let kind_attribute = attributes[0];
                    let job_id_attribute = attributes[1];
                    assert_eq!(kind_attribute.0, "kind");
                    assert_eq!(job_id_attribute.0, "job_id");
                    if let AttributeValue::Str(kind) = kind_attribute.1 {
                        assert_eq!(*kind, "total");
                    } else {
                        assert!(false, "kind attribute should be of str type");
                    }
                    if let AttributeValue::String(job_id) = job_id_attribute.1 {
                        assert_eq!(job_id, "5678");
                    } else {
                        assert!(false, "job_id attribute should be of string type");
                    }
                    let memory_usage = measurements.next().unwrap();
                    assert_eq!(cpu_time_delta.value, WrappedMeasurementValue::U64(10));
                    assert_eq!(memory_usage.value, WrappedMeasurementValue::U64(987654321));
                    let cpu_time_delta_path = format!("{}", cpu_time_delta.consumer.id_display());
                    assert_eq!(
                        cpu_time_delta_path,
                        format!(
                            "{0}/cpuacct/oar/somesubmitter_5678/cpuacct.usage",
                            path.to_str().unwrap()
                        )
                    );
                    let memory_usage_consumer_path = format!("{}", memory_usage.consumer.id_display());
                    assert_eq!(
                        memory_usage_consumer_path,
                        format!(
                            "{0}/memory/oar/somesubmitter_5678/memory.usage_in_bytes",
                            path.to_str().unwrap()
                        )
                    );
                },
            )
            .test_source(
                SourceName::from_str("oar2-plugin", "anewsubmitter_666"),
                || (),
                move |m| {
                    assert_eq!(m.len(), 1);
                    let memory_usage = m.iter().next().unwrap();
                    assert_eq!(memory_usage.value, WrappedMeasurementValue::U64(321321321));
                },
            );

        let agent = agent::Builder::new(plugins)
            .with_expectations(runtime_expectations)
            .build_and_start()
            .unwrap();

        agent.wait_for_shutdown(Duration::from_secs(5)).unwrap();

        Ok(())
    }
    fn config_to_toml_table(config: &Oar2Config) -> toml::Table {
        toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
    }

    enum CgroupFile {
        CpuAcctUsage(u64),
        MemoryUsageInBytes(u64),
    }

    impl CgroupFile {
        fn controller(&self) -> &'static str {
            match self {
                CgroupFile::CpuAcctUsage(_) => "cpuacct",
                CgroupFile::MemoryUsageInBytes(_) => "memory",
            }
        }
        fn file_name(&self) -> &'static str {
            match self {
                CgroupFile::CpuAcctUsage(_) => "cpuacct.usage",
                CgroupFile::MemoryUsageInBytes(_) => "memory.usage_in_bytes",
            }
        }

        fn write(&self, dir: &Path) -> io::Result<()> {
            let file_path = dir.join(self.file_name());
            let mut file = File::create(file_path)?;
            match self {
                CgroupFile::CpuAcctUsage(val) | CgroupFile::MemoryUsageInBytes(val) => {
                    writeln!(file, "{}", val)?;
                }
            }
            Ok(())
        }
    }

    /// Examples of Cgroup OAR layout
    /// /sys/fs/cgroup/cpuacct/oar/submitter1_1234/cpuacct.usage
    /// /sys/fs/cgroup/cpuacct/oar/submitter2_1234/cpuacct.usage
    /// /sys/fs/cgroup/memory/oar/submitter1_1234/memory.usage_in_bytes
    /// /sys/fs/cgroup/memory/oar/submitter2_1234/memory.usage_in_bytes
    struct OarMock<'a> {
        submitter: &'a str,
        job_id: &'a str,
        cgroup_files: Vec<CgroupFile>,
    }
    fn create_oar_mocks(base_path: &Path, mocks: Vec<OarMock>) -> io::Result<()> {
        for mock in mocks {
            for cgroup_file in mock.cgroup_files {
                let cgroup_path = base_path
                    .join(cgroup_file.controller())
                    .join("oar")
                    .join(format!("{0}_{1}", mock.submitter, mock.job_id));
                fs::create_dir_all(&cgroup_path)?;
                cgroup_file.write(&cgroup_path)?;
            }
        }
        Ok(())
    }
}
