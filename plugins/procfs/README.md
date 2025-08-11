# Procfs plugin

Collects processes and system-related metrics by reading the [proc](https://www.kernel.org/doc/html/latest/filesystems/proc.html) virtual filesystem on Linux based operating systems.

## Requirements

- Linux operating system.
- Read access to the `/proc` virtual file system. Depending on the mount options, [some privileges might be needed](#procfs-access).

## Metrics

There are various information collected by this plugin relative to Kernel, CPU, memory and processes:

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`kernel_cpu_time`|CounterDiff|millisecond|Time during the CPU is busy|LocalMachine|LocalMachine|[cpu_state](#cpu_state)|
|`kernel_context_switches`|CounterDiff|none|Number of context switches*|LocalMachine|LocalMachine||
|`kernel_new_forks`|CounterDiff|none|Number of forked operations*|LocalMachine|LocalMachine||
|`kernel_n_procs_running`|Gauge|none|Number of processes in a runnable state|LocalMachine|LocalMachine||
|`kernel_n_procs_blocked`|Gauge|none|Numbers of processes that are blocked on input/output operations|LocalMachine|LocalMachine||
|`cpu_time_delta`|CounterDiff|millisecond|CPU usage|LocalMachine|Process|[kind](#kind)|
|`memory_usage`|Gauge|bytes|Memory usage|LocalMachine|Process|[kind](#kind)|

- ***Context switches**: Operation allowing a single CPU to manage multiple processes efficiently, involves saving the state of a currently running process and loading the state of another process, enabling multitasking and optimal CPU utilization.
- ***Forks**: When a process creates a copy of itself.

### Attributes

#### Kind

The kind of the memory is the allocated memory space reserved by the system or the hardware (https://man7.org/linux/man-pages/man5/proc_pid_status.5.html):

|Value|Description|
|-----|-----------|
|`resident`|Resident set size (same as VmRSS in `/proc/<pid>/status`)|
|`shared`|Number of resident shared pages (i.e., backed by a file) (same as RssFile+RssShmem in `/proc/<pid>/status`)|
|`virtual`|Virtual memory size (same as VmSize in `/proc/<pid>/status`)|

The kind of the CPU time delta is the average CPU time spent by various tasks:

|Value|Description|
|-----|-----------|
|`user`|Time spent in user mode|
|`system`|Time spent in system mode|
|`guest`|Time spent running a virtual CPU for guest operating systems under control of the linux kernel|

#### cpu_state

The CPU states is an attribute that indicates the kind of cpu time that is measured:

|Value|Description|
|-----|-----------|
|`user`|Time spent in user mode|
|`nice`|Time spent in user mode with low priority (nice)|
|`system`|Time spent in system mode|
|`idle`|Time spent in the idle state|
|`irq`|Time servicing interrupts|
|`softirq`|Time servicing soft interrupts|
|`steal`|Time of stolen time. Stolen time is the time spent in other operating systems when running in a virtualized environment.|
|`guest`|Time spent running a virtual CPU for guest operating systems under control of the linux kernel|
|`guest_nice`|Time spent running a niced guest|

## Configuration

Here is a configuration example of the plugin. It is composed of different sections. Each section can be enabled or disabled with the `enabled` boolean parameter.

### Kernel metrics

To active the plugin to collect metrics relative to the kernel utilization:

```toml
[plugins.procfs.kernel]
# `true` to enable the monitoring of kernel information.
enabled = true
# How frequently should the kernel information be flushed to the rest of the pipeline.
poll_interval = "5s"
```

### Memory metrics

Moreover, you can collect more or less precise metrics on memory consumption, by setting the level of detail you want to extract from `/proc/meminfo` file (refers to https://man7.org/linux/man-pages/man5/proc_meminfo.5.html). The names of the collected metrics are converted to snake case (`MemTotal` becomes `mem_total`):

```toml
[plugins.procfs.memory]
# `true` to enable the monitoring of memory information.
enabled = true
# How frequently should the memory information be flushed to the rest of the pipeline.
poll_interval = "5s"
# The entry to parse from `/proc/meminfo`.
metrics = [
    "MemTotal",
    "MemFree",
    "MemAvailable",
    "Cached",
    "SwapCached",
    "Active",
    "Inactive",
    "Mapped",
]
```

### Process metrics

To enable process monitoring, you need to set the metrics collect policy via a `strategy`:

- `watcher`: Default strategy of system watcher to collect new processes, whatever it may be.
- `event`: Set this parameter to collect the process that acts as an internal event of ALUMET.

```toml
[plugins.procfs.processes]
# `true` to enable the monitoring of processes.
enabled = true
# Watcher refresh interval.
refresh_interval = "2s"
# `true` to watch for new processes, `false` to only react to ALUMET events.
strategy = "watcher"
```

### Group process metrics

Also, you can monitor groups of processes, i.e. processes defined by common characteristics. The available filters are `pid` (process id), `ppid` (parent process id) and `exe_regex` (a regular expression that must match the process executable path):

```toml
[[plugins.procfs.processes.groups]]
# Only monitor the processes whose executable path matches this regex.
exe_regex = ""
# How frequently should the processes information be refreshed.
poll_interval = "2s"
# How frequently should the processes information be flushed to the rest of the pipeline.
flush_interval = "4s"
```

## More information

### Procfs Access

To grant the required access to retrieve all metrics properly by reading `/proc` filesystem, you need to configure the parameter [hidepid](https://docs.kernel.org/filesystems/proc.html#mount-options) by editing the configuration file `/proc/mounts`. This setting is a mount option for the `/proc` filesystem, that is used to control the visibility of processes to unprivileged users. In this way, it can define the access restriction to `/proc/<pid>/` directories, and therefore visibility of processes stats.
By default, the `hidepid` parameter is generally set to allow the full access to `/proc/<pid>/` directories on Linux systems. If your system was configured differently, you must edit the configuration file `/proc/mounts`, and root privileges may be required for this operation.

```bash
mount -o remount,hidepid=0 -t proc proc /proc
```

Which results in a remount on the /proc mount point with full visibility of all user processes on the system. If you want to set a precise visibility, here are its available configuration values:

|Value|Description|
|-----|-----------|
|0|`default`: Everybody may access all `/proc/<pid>/` directories|
|1|`noaccess`: Users may not access any `/proc/<pid>/` directories but their own|
|2|`invisible`: All `/proc/<pid>/` will be fully invisible to other users|
|4|`ptraceable`: Procfs should only contain `/proc/<pid>/` directories that the caller can ptrace. The capability `CAP_SYS_PTRACE` may be required for PTraceable configuration|
