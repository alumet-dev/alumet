# Containers Plugin

The `containers` plugin provides cgroup measurements with container annotations using OCI-compliant runtimes APIs (docker and podman for now) which are automatically detected via the bollard library.

## Requirements

You need:
1. Docker or Podman running and accessible via their API
2. In the case of Podman, system service API needs to be enabled for the appropiate user: <https://www.redhat.com/en/blog/podman-rest-api>.

See the OCI Runtime Specification for more information: <https://github.com/opencontainers/runtime-spec>

## Metrics

Here are the metrics collected by the plugin's sources.

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`cpu_time_delta`|CounterDiff|nanoseconds|time spent by the container executing on the CPU|`LocalMachine`|`Cgroup`|see below|
|`cpu_percent`|Gauge|Percent (0 to 100)|`cpu_time_delta / delta_t / n_cores` (all cores used fully = 100%)|`LocalMachine`|`Cgroup`|see below|
|`memory_usage`|Gauge|Bytes|total container's memory usage|`LocalMachine`|`Cgroup`|see below|
|`memory_max`|Gauge|Bytes|Maximum memory available for the cgroup|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_anonymous`|Gauge|Bytes|anonymous memory usage|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_file`|Gauge|Bytes|memory used to cache filesystem data|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_kernel_stack`|Gauge|Bytes|memory allocated to kernel stacks|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_pagetables`|Gauge|Bytes|memory reserved for the page tables|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_swap_current`|Gauge|Bytes|current swap usage of the cgroup|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_swap_max`|Gauge|Bytes|maximum allowed swap usage for the cgroup|`LocalMachine`|`Cgroup`|see below|
|`cgroup_slab_reclaimable`|Gauge|Bytes|Amount of reclaimable kernel slab memory used by the cgroup.|`LocalMachine`|`Cgroup`|see below|
|`cgroup_pswpin`|CounterDiff|Pages|Total number of pages swapped into memory by the cgroup.|`LocalMachine`|`Cgroup`|see below|
|`cgroup_pswpout`|CounterDiff|Pages|Total number of pages swapped out of memory by the cgroup.|`LocalMachine`|`Cgroup`|see below|
|`cgroup_pswpin_bytes`|CounterDiff|Bytes|Total size of pages swapped into memory by the cgroup.|`LocalMachine`|`Cgroup`|see below|
|`cgroup_pswpout_bytes`|CounterDiff|Bytes|Total size of pages swapped out of memory by the cgroup.|`LocalMachine`|`Cgroup`|see below|
|`io_pressure_some_total`|CounterDiff|microseconds|IO pressure some total delta (at least one task stalled)|`LocalMachine`|`Cgroup`|none|
|`io_pressure_full_total`|CounterDiff|microseconds|IO pressure full total delta (all tasks stalled)|`LocalMachine`|`Cgroup`|none|
|`cpuset_cpus_effective`|Gauge|Unit|Number of effective logical CPUs available to the cgroup|`LocalMachine`|`Cgroup`|none|

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
[plugins.containers]
# Interval between each measurement
poll_interval = "5s"

# If `true`, adds attributes like `uid`, `name` to the cgroup measurements produced by other plugins
annotate_foreign_measurements = false
```
