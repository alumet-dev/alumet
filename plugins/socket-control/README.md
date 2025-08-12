# Socket Control plugin

This plugin allows to control the Alumet pipeline through a Unix socket (it could be extended to support other forms of communications).

## How to use

Run `app-agent` (or another app that loads this plugin) and open another terminal to send commands to the socket.

Terminal 1:

```sh
cd app-agent
cargo run
```

Terminal 2:

```sh
cd app-agent
echo "source trigger every 2s" | socat UNIX-CONNECT:./alumet-control.sock -
```
