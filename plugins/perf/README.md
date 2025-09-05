# Perf plugin

The perf plugin creates an Alumet **source** that collects measurements using the Performance Counters for Linux (aka `perf_events`).
It can obtain valuable data about the system and/or a specific process, such as the number of instructions executed, cache-misses suffered, …
This plugin works in a similar way to the [`perf` command-line tool](https://man7.org/linux/man-pages/man1/perf.1.html).

## Requirements

- Linux (`perf_events` is a kernel feature)
- [Required capabilities](#perf_event_paranoid-and-capabilities).

## Metrics

Here are the metrics collected by the plugin's source.
All the metrics are counters.

To learn more about the standard events, please refer to the [`perf_event_open` manual](https://man7.org/linux/man-pages/man2/perf_event_open.2.html).
To list the events that are available on your machine, run the `perf list` command.

**For hardware related metrics:**

`perf_hardware_{hardware-event-name}` where `hardware-event-name` is one of:

`CPU_CYCLES`, `INSTRUCTIONS`, `CACHE_REFERENCES`, `CACHE_MISSES`, `BRANCH_INSTRUCTIONS`, `BRANCH_MISSES`, `BUS_CYCLES`, `STALLED_CYCLES_FRONTEND`, `STALLED_CYCLES_BACKEND`, `REF_CPU_CYCLES`.

**For software related metrics:**

`perf_software_{software-event-name}` where `software-event-name` is one of:

`PAGE_FAULTS`, `CONTEXT_SWITCHES`, `CPU_MIGRATIONS`, `PAGE_FAULTS_MIN`, `PAGE_FAULTS_MAJ`, `ALIGNMENT_FAULTS`, `EMULATION_FAULTS`, `CGROUP_SWITCHES`.

**For cache related metrics:**

`perf_cache_{cache-id}_{cache-op}_{cache-result}` where:

`cache-id` is one of `L1D`, `L1I`, `LL`, `DTLB`, `ITLB`, `BPU`, `NODE`

`cache-op` is one of `READ`, `WRITE` or `PREFETCH`.

`cache-result` is one of `ACCESS` or `MISS`.

Note that based on your kernel version, some events could be unavailable.

### Attributes

## Configuration

Here is a configuration example of the plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.perf]
# Description.
poll_interval = "1s"
flush_interval = "1s"
hardware_events = [
    "REF_CPU_CYCLES",
    "CACHE_MISSES",
    "BRANCH_MISSES",
#   // any {hardware-event-name} from the list previously mentionned
]
software_events = [
    "PAGE_FAULTS",
    "CONTEXT_SWITCH",
#   // any {software-event-name} from the list previously mentionned
]
cache_events = [
    "LL_READ_MISS",
#   // any combination of {cache-id}_{cache-op}_{cache-result} from the lists previously mentionned
]
```

## More information

### perf_event_paranoid and capabilities

| `perf_event_paranoid` value     | Description                                            | Required capabilities (binary)                       | `perf` plugin works (unprivileged) |
Below is a summary of how different perf_event_paranoid values affect perf plugin functionality when running as an unprivileged user:

| `perf_event_paranoid` value     | Description                                            | Required capabilities (binary)                       | RAPL plugin works (unprivileged) |
| ------------------------------- | ------------------------------------------------------ | ---------------------------------------------------- | -------------------------------- |
| 4 *(Debian-based systems only)* | Disables all perf event usage for unprivileged users   | −                                                    | ❌ Not supported                 |
| 2                               | Allows only user-space measurements                    | `cap_perfmon` *(or `cap_sys_admin` for Linux < 5.8)* | ✅ Supported                     |
| 1                               | Allows user-space and kernel-space measurements        | `cap_perfmon` *(or `cap_sys_admin` for Linux < 5.8)* | ✅ Supported                     |
| 0                               | Allows user-space, kernel-space, and CPU-specific data | `cap_perfmon` *(or `cap_sys_admin` for Linux < 5.8)* | ✅ Supported                     |
| -1                              | Full access, including raw tracepoints                 | −                                                    | ✅ Supported                     |

Example for setting `perf_event_paranoid`: `sudo sysctl -w kernel.perf_event_paranoid=2` will set the value to **2**.

Note that this command will not make it permanent (reset after restart).
To make it permanent, create a configuration file in `/etc/sysctl.d/` (this may change depending on your Linux distro).

Alternatively, you can run Alumet as a **privileged user** (root), but this is not recommended for security reasons.
