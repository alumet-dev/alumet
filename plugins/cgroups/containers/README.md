# Containers Plugin

The `containers` plugin provides cgroup measurements with container annotations using OCI-compliant runtimes APIs (docker and podman for now) which are automatically detected via the bollard library.

## Requirements

You need:
1. Docker or Podman running and accessible via their API

See the OCI Runtime Specification for more information: https://github.com/opencontainers/runtime-spec

## Metrics

Those from ../util-cgroups-plugins/src/metrics.rs

### Attributes

The measurements produced by the `containers` plugin have the following attributes:
- `uid`: the container's unique identifier
- `name`: the container's name

## Annotation of the Measurements Provided by Other Plugins

Other plugins, such as the [`process-to-cgroup-bridge`](../../process-to-cgroup-bridge/README.md), can produce measurements related to the cgroups of containers.
However, they cannot add container-specific information (such as the container UID or name) to the measurements.

To do that, use the annotation feature of the `containers` plugin by enabling the following configuration option.

```toml
annotate_foreign_measurements = true
```

Be sure to enable the `containers` plugin **after** the plugins that produce the measurements that you want to annotate.
For instance, the `containers` configuration section should be after the `process-to-cgroup-bridge` section.

```toml
[plugins.process-to-cgroup-bridge]
…

[plugins.containers]
…
```

## Configuration

```toml
[[plugins]]
name = "containers"

[plugins.containers]
# Interval between each measurement
poll_interval = "5s"

# If `true`, adds attributes like `uid`, `name` to the cgroup measurements produced by other plugins
annotate_foreign_measurements = false
```
