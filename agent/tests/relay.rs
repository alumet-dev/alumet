//! Integration tests for the relay mode, client and server together.
mod common;

use anyhow::{anyhow, Context};
use std::{
    process::{self, ExitStatus, Stdio},
    time::Duration,
};

use common::{
    empty_temp_dir,
    run::{command_run_agent, run_agent_tee, ChildGuard},
};

/// Checks that the `--relay-server` option works when the relay-client plugin is enabled.
#[test]
fn args_bad_relay_server_address() {
    let tmp_dir = empty_temp_dir("args_bad_relay_server_address").unwrap();
    let out = run_agent_tee(
        "alumet-agent",
        &[
            "--plugins",
            "relay-client",
            "--relay-out",
            "BADuri:#é",
            "exec",
            "sleep 1",
        ],
        &tmp_dir,
    )
    .expect("failed to run the client and capture its output");
    assert!(
        !out.status.success(),
        "Alumet relay client should fail because of the bad relay server address"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    let msg = "BADuri:#é";
    assert!(
        stderr.contains(msg) || stdout.contains(msg),
        "The incorrect URI should show up in the logs"
    );
}

/// Checks that the client can send measurements to the server,
/// which will write them to a CSV file.
///
/// Note: we use a limited set of plugins so that it works in the CI environment.
#[test]
fn client_to_server_to_csv() {
    // These tests are in the same test function because they must NOT run concurrently (same port).

    // works in CI
    client_to_server_to_csv_on_address("ipv4", Some("localhost:50051")).unwrap();

    // doesn't work in CI
    if std::env::var_os("NO_IPV6").is_some() {
        println!("IPv6 test disabled by environment variable.");
    } else {
        client_to_server_to_csv_on_address("ipv6", Some("[::1]:50051")).unwrap();
        client_to_server_to_csv_on_address("default", None).unwrap();
    }
}

fn client_to_server_to_csv_on_address(tag: &str, addr_and_port: Option<&'static str>) -> anyhow::Result<()> {
    let tmp_dir = empty_temp_dir(&format!("client_to_server_to_csv-{tag}"))?;

    let server_config = tmp_dir.join("server.toml");
    let client_config = tmp_dir.join("client.toml");
    let server_output = tmp_dir.join("output.csv");
    assert!(
        matches!(&server_config.try_exists(), Ok(false)),
        "server config should not exist"
    );
    assert!(
        matches!(&client_config.try_exists(), Ok(false)),
        "client config should not exist"
    );
    assert!(
        matches!(&server_output.try_exists(), Ok(false)),
        "server output file should not exist"
    );

    let server_config_str = server_config.to_str().unwrap().to_owned();
    let client_config_str = client_config.to_str().unwrap().to_owned();
    let server_output_str = server_output.to_str().unwrap().to_owned();

    // Spawn the server
    let server_csv_output_conf = format!("plugins.csv.output_path='''{server_output_str}'''");
    let mut server_args = Vec::from_iter([
        "--config",
        &server_config_str,
        // only enable some plugins
        "--plugins=relay-server,csv",
        // ensure that the CSV plugin flushes the buffer to the file ASAP
        "--config-override",
        "plugins.csv.force_flush=true",
        // set the CSV output to the file we want
        "--config-override",
        &server_csv_output_conf,
    ]);
    if let Some(addr_and_port) = addr_and_port {
        server_args.extend_from_slice(&["--relay-in", addr_and_port]);
    }
    let server_process: process::Child = command_run_agent("alumet-agent", &server_args)?
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("server process should spawn")?;
    let mut server_process = ChildGuard::new(server_process);
    println!("spawned server process {}", server_process.id());

    // Wait for the server to start
    let mut loop_limit = 500;
    while !std::fs::exists(&server_config).context("could not check existence of config")? {
        if loop_limit == 0 {
            let _ = server_process.kill();
            panic!("The server config is not generated! Config path: {server_config_str}");
        }
        std::thread::sleep(Duration::from_millis(100));
        loop_limit -= 1;
    }
    std::thread::sleep(Duration::from_millis(250));

    // Start the client
    let mut client_args: Vec<String> = Vec::from_iter([
        // use a different config than the server
        "--config",
        &client_config_str,
        // only enable some plugins
        "--plugins=relay-client,procfs",
        // override the config to lower the poll_interval (so that the test is faster)
        "--config-override",
        "plugins.procfs.kernel.poll_interval='50ms'",
        "--config-override",
        "plugins.procfs.memory.poll_interval='50ms'",
        "--config-override",
        "plugins.procfs.processes.enabled=false",
        // don't buffer the relay output (because we want to check the final output after a short delay)
        "--config-override",
        "plugins.relay-client.buffer_max_length=0",
    ])
    .into_iter()
    .map(String::from)
    .collect();

    if let Some(addr_and_port) = addr_and_port {
        client_args.extend_from_slice(&[
            // specify an URI that works in the CI
            "--relay-out".into(),
            addr_and_port.into(),
        ]);
    }
    let client_args: Vec<&str> = client_args.iter().map(|s| s.as_str()).collect();

    let client_process = command_run_agent("alumet-agent", &client_args)?
        // .stdout(Stdio::piped())
        // .stderr(Stdio::piped())
        .env("RUST_LOG", "debug")
        .spawn()?;
    let mut client_process = ChildGuard::new(client_process);
    println!("spawned client process {}", client_process.id());

    // Wait a little bit
    let delta = Duration::from_millis(1000);
    std::thread::sleep(delta);

    // Check that the processes still run
    assert!(
        matches!(client_process.try_wait(), Ok(None)),
        "the client should still run after a while"
    );
    assert!(
        matches!(server_process.try_wait(), Ok(None)),
        "the server should still run after a while"
    );

    // Check that we've obtained some measurements
    // let output_content_before_stop = std::fs::read_to_string(&server_output)?;
    // assert!(
    //     !output_content_before_stop.is_empty(),
    //     "some measurements should have been written after {delta:?}"
    // );

    // Stop the client
    kill_gracefully(&mut client_process)?;

    // Wait for the client to stop (TODO: a timeout would be nice, but it's no so simple to have)
    let client_status = client_process.take().wait()?;
    assert!(
        stopped_gracefully(client_status),
        "the client should exit in a controlled way, but had status {client_status}"
    );

    // Check that we still have measurements
    let output_content_after_stop = std::fs::read_to_string(&server_output)?;
    assert!(
        !output_content_after_stop.is_empty(),
        "some measurements should have been written after the client shutdown"
    );

    // Stop the server
    kill_gracefully(&mut server_process)?;

    // Wait for the server to be stopped.
    let server_output = server_process.take().wait_with_output()?;
    let server_status = server_output.status;
    println!(
        "vvvvvvvvvvvv server output below vvvvvvvvvvvv\n{}\n------\n{}\n------\n",
        String::from_utf8(server_output.stdout).unwrap(),
        String::from_utf8(server_output.stderr).unwrap()
    );
    assert!(
        stopped_gracefully(server_status),
        "the server should exit in a controlled way, but had status {server_status}"
    );
    Ok(())
}

fn stopped_gracefully(status: ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    status.success() || status.signal().is_some()
}

fn kill_gracefully(child: &mut process::Child) -> anyhow::Result<()> {
    let res = unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    if res == 0 {
        Ok(())
    } else {
        Err(anyhow!("failed to kill process {}", child.id()))
    }
}
