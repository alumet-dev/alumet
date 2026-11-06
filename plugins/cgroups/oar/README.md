# OAR plugin

The `oar` plugin gathers measurements about [OAR](https://oar.imag.fr/) jobs.

## Requirements

- A node with OAR installed and running. Both OAR 2 and OAR 3 are supported ([config required](#configuration)).
- Both cgroups v1 and cgroups v2 are supported. Some metrics may not be available with cgroups v1.

## Metrics

Here are the metrics collected by the plugin's sources.

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`cpu_time_delta`|CounterDiff|nanoseconds|time spent by the job executing on the CPU|`LocalMachine`|`Cgroup`|see below|
|`cpu_percent`|Gauge|Percent (0 to 100)|`cpu_time_delta / delta_t / n_cores` (all cores used fully = 100%)|`LocalMachine`|`Cgroup`|see below|
|`memory_usage`|Gauge|Bytes|total job's memory usage|`LocalMachine`|`Cgroup`|see below|
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

The measurements produced by the `slurm` plugin have the following attributes:
- `job_id`: id of the OAR job.
- `user_id`: id of the user that submitted the job.

The **cpu** measurements have an additional attribute `kind`, which can be one of:
- `total`: time spent in kernel and user mode
- `system`: time spent in kernel mode only
- `user`: time spent in user mode only

## Augmentation of the measurements of other plugins

The `oar` plugin adds attributes to the measurements of the other plugins.
If a measurement does not have a `job_id` attribute, it gets a new `involved_jobs` attribute, which contains a list of the ids of the jobs that are running on the node (at the time of the transformation).

This allows to know, for each measurement, which job was running at that time.
For the reasoning behind this feature, see [issue #209](https://github.com/alumet-dev/alumet/issues/209).

## Annotation of the Measurements Provided by Other Plugins

Other plugins, such as the [`process-to-cgroup-bridge`](../../process-to-cgroup-bridge/README.md), can produce measurements related to the cgroups of OAR jobs.
However, they cannot add job-specific information (such as the job id) to the measurements.

To do that, use the annotation feature of the `oar` plugin by enabling the following configuration option.

```toml
annotate_foreign_measurements = true
```

Be sure to enable the `oar` plugin **after** the plugins that produce the measurements that you want to annotate.
For instance, the `oar` configuration section should be after the `process-to-cgroup-bridge` section.

```toml
[plugins.process-to-cgroup-bridge]
…

[plugins.oar]
…
```

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.oar]
# The version of OAR, either "oar2" or "oar3".
oar_version = "oar3"
# Interval between each measurement.
poll_interval = "1s"
# Disable OAR measurements. If true, no sources will be created.
# This is useful if you only need to use a subpart of the plugin such as cgroups observer or annotation transform.
disable_sources = false
# If true, only monitors jobs and ignore other cgroups.
jobs_only = true
```
