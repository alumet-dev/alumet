use std::{io::Write, os::unix::net::UnixStream, time::Duration};

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    plugin::{rust::serialize_config, PluginMetadata},
};
use plugin_socket_control::{Config, SocketControlPlugin};

#[test]
fn shutdown() {
    // for debugging
    env_logger::init();

    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    let socket_file = tmp.path().join("control.sock");

    let plugin_config = serialize_config(Config {
        socket_path: socket_file.to_str().unwrap().to_owned(),
    })
    .unwrap()
    .0;

    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<SocketControlPlugin>(),
        enabled: true,
        config: Some(plugin_config),
    });

    let agent = agent::Builder::new(plugins)
        .build_and_start()
        .expect("alumet should start");

    // wait a bit, so that the socket is visible
    std::thread::sleep(Duration::from_millis(100));

    // send a command to the socket
    let mut stream = UnixStream::connect(socket_file).expect("I should be able to connect to the socket");
    socket_write_line(&mut stream, "control source stop"); // just to check the "hard" path of command execution
    socket_write_line(&mut stream, "shutdown");

    // check that alumet has stopped
    agent
        .wait_for_shutdown(Duration::from_millis(250))
        .expect("alumet should stop");
}

fn socket_write_line(stream: &mut UnixStream, line: &str) {
    let buf = format!("{line}\n").into_bytes();
    // the newline is important, because the plugin uses read_line() to parse the commands
    stream.write_all(&buf).expect("I should be able to write to the socket");
    stream.flush().unwrap();
}
