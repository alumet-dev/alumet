# Raw cgroups plugin

The `cgroups` plugin gathers measurements about Linux control groups.

## Requirements

- Control groups [v1](https://docs.kernel.org/admin-guide/cgroup-v1/cgroups.html) or [v2](https://docs.kernel.org/admin-guide/cgroup-v2.html). Some metrics may not be available with cgroups v1.

## Metrics

Here are the metrics collected by the plugin's sources.

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`cpu_time_delta`|CounterDiff|nanoseconds|time spent by the cgroups executing on the CPU|`LocalMachine`|`Cgroup`|see below|
|`cpu_percent`|Gauge|Percent (0 to 100)|`cpu_time_delta / delta_t / n_cores` (all cores used fully = 100%)|`LocalMachine`|`Cgroup`|see below|
|`memory_usage`|Gauge|Bytes|total cgroups's memory usage|`LocalMachine`|`Cgroup`|see below|
|`memory_max`|Gauge|Bytes|Maximum memory available for the cgroup|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_anonymous`|Gauge|Bytes|anonymous memory usage|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_file`|Gauge|Bytes|memory used to cache filesystem data|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_kernel_stack`|Gauge|Bytes|memory allocated to kernel stacks|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_pagetables`|Gauge|Bytes|memory reserved for the page tables|`LocalMachine`|`Cgroup`|see below|
|`cgroup_slab_reclaimable`|Gauge|Bytes|Amount of reclaimable kernel slab memory used by the cgroup.|`LocalMachine`|`Cgroup`|see below|
|`cgroup_pswpin`|Counter|Pages|Total number of pages swapped into memory by the cgroup.|`LocalMachine`|`Cgroup`|see below|
|`cgroup_pswpout`|Counter|Pages|Total number of pages swapped out of memory by the cgroup.|`LocalMachine`|`Cgroup`|see below|
|`io_pressure_some_total`|CounterDiff|microseconds|IO pressure some total delta (at least one task stalled)|`LocalMachine`|`Cgroup`|none|
|`io_pressure_full_total`|CounterDiff|microseconds|IO pressure full total delta (all tasks stalled)|`LocalMachine`|`Cgroup`|none|

### Attributes

The **cpu** measurements have an additional attribute `kind`, which can be one of:
- `total`: time spent in kernel and user mode
- `system`: time spent in kernel mode only
- `user`: time spent in user mode only

## Special Values for Maximum Metrics

Some metrics that represent maximum limits (`memory_max`, `memory_swap_max`) are returned as `f64` instead of `u64` to handle a special case:

- When these metrics return `-1`, it indicates that the capping is set to maximum (unlimited), meaning the actual limit value is not available or meaningful. This occurs when the cgroup has no explicit limit set and can use all available system resources.

For all other values, these metrics represent the actual limit in bytes.

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.cgroups]
# Interval between each measurement.
poll_interval = "1s"
# Disable cgroups measurements. If true, no sources will be created.
# This is useful if you only need to use a subpart of the plugin such as cgroups observer or annotation transform.
disable_sources = false
```

## Automatic Detection

The version of the control groups and the mount point of the cgroupfs are automatically detected.

The plugin watches for the creation and deletion of cgroups.
With cgroup v2, the detection is almost instantaneous, because it relies on inotify.
With cgroup v1, however, cgroups are repeatedly polled. The refresh interval is `30s`, and it is currently not possible to change it in the plugin's configuration.

## More information

To monitor HPC jobs or Kubernetes pods, use the [OAR](../oar/README.md), [Slurm](../slurm/README.md) or [K8S](../k8s/README.md) plugins.
They provide more information about the jobs/pods, such as their id.
