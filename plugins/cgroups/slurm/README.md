# Slurm plugin

The `slurm` plugin gathers measurements about Slurm jobs.

## Requirements

- A node with Slurm installed and running.
- The Slurm plugin relies on cgroups for its operation. Knowing that, your slurm cluster should have the cgroups enabled. Here is the [official documentation](https://slurm.schedmd.com/cgroups.html) about how to setup this.

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

The measurements produced by the `slurm` plugin have the following attributes:
- `job_id`: id of the Slurm job, for example `10707`.
- `job_step`: id of the Slurm job, for example `2` (the full job id with its step is `10707.2` and the `job_step` attribute contains only the step number `2`).

The **cpu** measurements have an additional attribute `kind`, which can be one of:
- `total`: time spent in kernel and user mode
- `system`: time spent in kernel mode only
- `user`: time spent in user mode only

## Annotation of the Measurements Provided by Other Plugins

Other plugins, such as the [`process-to-cgroup-bridge`](../../process-to-cgroup-bridge/README.md), can produce measurements related to the cgroups of Slurm jobs.
However, they cannot add job-specific information (such as the job id) to the measurements.

To do that, use the annotation feature of the `slurm` plugin by enabling the following configuration option.

```toml
annotate_foreign_measurements = true
```

Be sure to enable the `slurm` plugin **after** the plugins that produce the measurements that you want to annotate.
For instance, the `slurm` configuration section should be after the `process-to-cgroup-bridge` section.

```toml
[plugins.process-to-cgroup-bridge]
…

[plugins.slurm]
…
```

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.slurm]
# Interval between two measurements
poll_interval = "1s"
# Interval between two scans of the cgroup v1 hierarchies.
# Only applies to cgroup v1 hierarchies (cgroupv2 supports inotify).
cgroupv1_refresh_interval = "30s"
# Only monitor the job cgroup related metrics and skip the others
jobs_only = true
# If true, the slurm sources will be started in pause state (only for advanced setup with a control plugin enabled)
add_source_in_pause_state = false
```
