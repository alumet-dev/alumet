use std::{io::Write, path::Path, time::Duration};

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
use plugin_k8s::K8sPlugin;
use util_cgroups::hierarchy::find_user_app_slice;

const SYSFS_CGROUP: &str = "/sys/fs/cgroup";
const TIMEOUT: Duration = Duration::from_secs(1);
const TOLERANCE: Duration = Duration::from_millis(500);
const POD_UID: &str = "00b506dc-87ee-462c-880d-3e41d0dacd0c";
const POD_NODE: &str = "test-node";
const TOKEN_CONTENT: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwiZXhwIjo0MTAyNDQ0ODAwLCJuYW1lIjoiVDNzdDFuZyBUMGszbiJ9.3vho4u0hx9QobMNbpDPvorWhTHsK9nSg2pZAGKxeVxA";

#[test]
fn test_k8s_cgroupv2() -> anyhow::Result<()> {
    if std::env::var_os("SKIP_CGROUPFS_TESTS").is_some() {
        println!("skipped because SKIP_CGROUPFS_TESTS is set");
        return Ok(());
    }

    let _ = env_logger::Builder::from_default_env().try_init();

    // find where we can create actual cgroups
    let app_slice = find_user_app_slice(Path::new(SYSFS_CGROUP))?;

    // find where to put the fake k8s token
    let mut token_file = tempfile::NamedTempFile::new()?;
    write!(&mut token_file, "{TOKEN_CONTENT}")?;
    let token_file_path = token_file.path().to_str().unwrap();

    // prepare fake k8s api
    let mut mock_server = mockito::Server::new();
    let mock_server_url = mock_server.url();
    let api_mock = mock_k8s_api_with_one_pod(&mut mock_server, 1);

    // load plugins
    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<K8sPlugin>(),
        enabled: true,
        config: Some(
            toml::from_str(&format!(
                r#"
                    poll_interval = "1s"
                    k8s_api_url = "{mock_server_url}"
                    k8s_node = "{POD_NODE}"
                    token_retrieval.file = "{token_file_path}"
                "#
            ))
            .unwrap(),
        ),
    });

    // start the measurement pipeline, without the pod's cgroup
    let agent = agent::Builder::new(plugins).build_and_start()?;
    std::thread::sleep(TOLERANCE);

    // create the cgroups
    let cgroup_dir_parent =
        tempfile::tempdir_in(&app_slice).with_context(|| format!("failed to create cgroup in {app_slice:?}"))?;
    let cgroup_dir_pod = cgroup_dir_parent.path().join(format!(
        "kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-pod{POD_UID}.slice"
    ));
    std::fs::create_dir_all(&cgroup_dir_pod)?;

    let source_name = &format!("kubepods-besteffort-pod{POD_UID}");
    log::info!("cgroup created at {:?}", cgroup_dir_pod);
    log::info!("source name: {source_name}");

    // expect the source to be created quickly after that
    std::thread::sleep(TOLERANCE);
    let handle = agent.pipeline.control_handle();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let elements = rt.block_on(
        handle.send_wait(
            request::list_elements(
                ElementListFilter::kind(ElementKind::Source)
                    .plugin("k8s")
                    .name(source_name),
            ),
            TIMEOUT,
        ),
    )?;
    let all_sources = rt.block_on(handle.send_wait(
        request::list_elements(ElementListFilter::kind(ElementKind::Source)),
        TIMEOUT,
    ))?;
    assert!(
        !elements.is_empty(),
        "source not found: {source_name}, all sources: {all_sources:?}"
    );

    // check that the API has been called the appropriate number of times
    api_mock.assert();

    // stop the pipeline and wait for it to terminate
    std::thread::sleep(Duration::from_secs(10));
    handle.shutdown();
    agent.wait_for_shutdown(TIMEOUT).context("error in shutdown")?;
    Ok(())
}

fn mock_k8s_api_with_one_pod(server: &mut mockito::Server, expected_hits: usize) -> mockito::Mock {
    server
        .mock("GET", "/api/v1/pods?fieldSelector=spec.nodeName%3Dtest-node")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(
            serde_json::json!({
                "items": [
                    {
                        "metadata": {
                            "name": "pod1",
                            "namespace": "default",
                            "uid": POD_UID,
                        },
                        "spec": {
                            "nodeName": POD_NODE
                        }
                    },
                ]
            })
            .to_string(),
        )
        .expect(expected_hits)
        .create()
}
