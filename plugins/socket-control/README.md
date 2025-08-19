# Socket Control plugin

This plugin allows to control the Alumet pipeline through a Unix socket (it could be extended to support other forms of communications).

## Requirements

- Linux
- Alumet agent must "have write and search (execute) permission on the directory in which the socket is created" as stated in the [`Pathname socket ownership and permissions` chapter from the unix man pages](https://man7.org/linux/man-pages/man7/unix.7.html).

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.socket-control]
socket_path = "alumet-control.sock"
```

## How to use

Once an alumet agent is running with the socket-control plugin loaded and enabled.
You can use the socket that is specified in the config file under the field `socket_path`.
To send a command through the socket to the agent, run:

```sh
echo "<command>" | socat UNIX-CONNECT:./alumet-control.sock -
```

### Available commands

- `shutdown` or `stop`: shutdowns the measurement pipeline
- `control <PATTERN> [ARGS...]`: reconfigures a part of the pipeline (see below)

#### Control patterns

The pattern has three levels of specification and can use wildcards.
You can either use the first level or the three levels together.
It must match with the following format `kind`/`plugin`/`element`, where:

- `kind` is the type of plugin that must be matched, it can be one of the three following: **source**, **output** or **transform**
- `plugin` is the name of the plugin that will be selected by the pattern, it can be "socket-control" for example
- `element` is the name of the element created by the plugin that match with the pattern, it can be the source name for a source plugin for example

Here are some valid examples:

```sh
# Match everything in the pipeline of alumet

*
*/*/*

# Match every output

output
output/*/*

# Match all the transforms of a plugin

transform/plugin-energy-attribution/*

# Match a specific source of a source plugin

source/plugin-procfs/memory
```

#### Control arguments

The available options for `control` depend on the kind of element that the selector targets.

Options available on any element (sources, transforms and outputs):

- `pause` or `disable`: pauses a source, transform or output
- `resume` or `enable`: resumes a source, transform or output

Options available on sources and outputs (not transforms):

- `stop`: stops and destroys the source or output

Options available on sources only:

- `set-period <Duration>`: changes the time period between two measurements (only works if the source is a "managed" source)
- `trigger-now`: requests Alumet to poll the source (only works if the source enables manual trigger)
