# Slurm plugin

The Slurm plugin creates some Alumet **source** that collect measurements of time used by the CPU and memory consumption through cgroups.

## Requirements

- A node with Slurm installed and running.
- The Slurm plugin relies on cgroups for its operation. Knowing that, your slurm cluster should have the cgroups enabled. Here is the [official documentation]("https://slurm.schedmd.com/cgroups.html") about how to setup this.

## Metrics

Here are the metrics collected by the plugin source.

| Name                           | Type         | Unit       | Description                                                                 | Resource | ResourceConsumer | Attributes  | More information |
| ------------------------------ | ------------ | ---------- | --------------------------------------------------------------------------- | -------- | ---------------- | ----------- | ---------------- |
| `cgroup_memory_anonymous_B`    | Gauge        | Byte       | Running process and various allocated memory measurement                    | Memory   | LocalMachine     | Job_id      |                  |
| `cgroup_memory_file_B`         | Gauge        | Byte       | Corresponding memory to open files and descriptors                          | Memory   | LocalMachine     | Job_id      |                  |
| `cgroup_memory_kernel_stack_B` | Gauge        | Byte       | Memory reserved for kernel operations                                       | Memory   | LocalMachine     | Job_id      |                  |
| `cgroup_memory_pagetables_B`   | Gauge        | Byte       | Memory used to manage correspondence between virtual and physical addresses | Memory   | LocalMachine     | Job_id      |                  |
| `cpu_percent_%`                | Gauge        | Percent    | Ratio of CPU used by the cgroup since last measurement                      | CPU      | LocalMachine     | Job_id,kind |                  |
| `cpu_time_delta_ns`            | Counter Diff | Nanosecond | Total CPU usage time by the cgroup since last measurement                   | CPU      | LocalMachine     | Job_id,kind |                  |
| `memory_usage_B`               | Gauge        | Byte       | Memory currently used by the cgroup                                         | Memory   | LocalMachine     | Job_id      |                  |

### Attributes

Here is a description of eah attribute:

- `Job_id`: Id of the job executed by Slurm.
- `kind`: Could be one of these three value:
  - `total`: Time spend by the processor in kernel and user mode to process the code belonging to the cgroup.
  - `system`: Time spend by the processor in kernel mode to process the code belonging to the cgroup.
  - `user`: Time spend by the processor in user mode to process the code belonging to the cgroup.

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.slurm]
# Interval between two Slurm measurement
poll_interval = "1s"
# Interval between two scans of the cgroup v1 hierarchies.
# Only applies to cgroup v1 hierarchies (cgroupv2 supports inotify).
cgroupv1_refresh_interval = "30s"
# Only monitor the job cgroup related metrics and skip the others
jobs_only = true
```

## More information

You can find more information about how to setup Slurm and Cgroups on the [official documentation]("https://slurm.schedmd.com/cgroups.html")
