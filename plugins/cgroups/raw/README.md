# Raw cgroups plugin

The `cgroups` plugin gathers measurements about Linux control groups.

## Requirements

- Control groups [v1](https://docs.kernel.org/admin-guide/cgroup-v1/cgroups.html) or [v2](https://docs.kernel.org/admin-guide/cgroup-v2.html). Some metrics may not be available with cgroups v1.

## Metrics

Here are the metrics collected by the plugin's sources.

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`cpu_time_delta`|Delta|nanoseconds|time spent by the pod executing on the CPU|`LocalMachine`|`Cgroup`|see below|
|`cpu_percent`|Gauge|Percent (0 to 100)|`cpu_time_delta / delta_t` (1 core used fully = 100%)|`LocalMachine`|`Cgroup`|see below|
|`memory_usage`|Gauge|Bytes|total pod's memory usage|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_anonymous`|Gauge|Bytes|anonymous memory usage|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_file`|Gauge|Bytes|memory used to cache filesystem data|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_kernel_stack`|Gauge|Bytes|memory allocated to kernel stacks|`LocalMachine`|`Cgroup`|see below|
|`cgroup_memory_pagetables`|Gauge|Bytes|memory reserved for the page tables|`LocalMachine`|`Cgroup`|see below|

### Attributes

The **cpu** measurements have an additional attribute `kind`, which can be one of:
- `total`: time spent in kernel and user mode
- `system`: time spent in kernel mode only
- `user`: time spent in user mode only

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.cgroups]
# Interval between each measurement.
poll_interval = "1s"
```

## Automatic Detection

The version of the control groups and the mount point of the cgroupfs are automatically detected.

The plugin watches for the creation and deletion of cgroups.
With cgroup v2, the detection is almost instantaneous, because it relies on inotify.
With cgroup v1, however, cgroups are repeatedly polled. The refresh interval is `30s`, and it is currently not possible to change it in the plugin's configuration.

## More information

To monitor HPC jobs or Kubernetes pods, use the [OAR](../oar/README.md), [Slurm](../slurm/README.md) or [K8S](../k8s/README.md) plugins.
They provide more information about the jobs/pods, such as their id.
