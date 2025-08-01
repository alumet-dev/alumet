use std::{path::Path, time::Duration};

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    pipeline::{
        control::request::{self, ElementListFilter},
        naming::ElementKind,
    },
    plugin::PluginMetadata,
};
use anyhow::Context;
use plugin_cgroup::plugins::{raw::Config, RawCgroupPlugin};
use util_cgroups::{hierarchy::find_user_app_slice, CgroupHierarchy, CgroupVersion};

const SYSFS_CGROUP: &str = "/sys/fs/cgroup";
const TIMEOUT: Duration = Duration::from_secs(1);
const TOLERANCE: Duration = Duration::from_millis(500);

#[test]
fn test_raw_cgroupv2() -> anyhow::Result<()> {
    if std::env::var_os("SKIP_CGROUPFS_TESTS").is_some() {
        println!("skipped because SKIP_CGROUPFS_TESTS is set");
        return Ok(());
    }

    let _ = env_logger::Builder::from_default_env().try_init();

    let app_slice = find_user_app_slice(Path::new(SYSFS_CGROUP))?;

    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<RawCgroupPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&Config {
            poll_interval: Duration::from_secs(1),
        })),
    });

    // start the measurement pipeline, without our "test_raw" cgroup
    let agent = agent::Builder::new(plugins).build_and_start()?;

    // create the cgroup
    let new_cgroup_dir =
        tempfile::tempdir_in(&app_slice).with_context(|| format!("failed to create cgroup in {app_slice:?}"))?;
    let cgroup_hierarchy = CgroupHierarchy::manually_unchecked(SYSFS_CGROUP, CgroupVersion::V2, vec!["cpu"]);
    let cgroup_path_in_hierarchy = cgroup_hierarchy.cgroup_path_from_fs(new_cgroup_dir.path()).unwrap();
    let source_name = cgroup_path_in_hierarchy.as_str();
    log::info!("cgroup created at {:?}", new_cgroup_dir.path());
    log::info!("source name: {source_name}");

    // expect the source to be created quickly after that
    std::thread::sleep(TOLERANCE);
    let handle = agent.pipeline.control_handle();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let elements = rt.block_on(
        handle.send_wait(
            request::list_elements(
                ElementListFilter::kind(ElementKind::Source)
                    .plugin("cgroups")
                    .name(source_name),
            ),
            TIMEOUT,
        ),
    )?;
    assert!(!elements.is_empty(), "source not found: {source_name}");

    // stop the pipeline and wait for it to terminate
    handle.shutdown();
    agent.wait_for_shutdown(TIMEOUT)?;
    Ok(())
}

fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().to_owned()
}
