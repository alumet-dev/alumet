# Perf plugin

The perf plugin creates an Alumet **source** that collects measurements using the Performance Counters for Linux (aka `perf_events`).
It can obtain valuable data about the system and/or a specific process, such as the number of instructions executed, cache-misses suffered, …
This plugin works in a similar way to the [`perf` command-line tool](https://man7.org/linux/man-pages/man1/perf.1.html).

## Requirements

- Linux (`perf_events` is a kernel feature)
- [Required capabilities](#perf_event_paranoid-and-capabilities).

## Events

Events to measure are described with a single unified syntax, following the `perf stat -e`
[event selection syntax](https://man7.org/linux/man-pages/man1/perf-stat.1.html). Each event is a
string of the form `<event>[:<modifiers>]`

### Event formats

The `<event>` part can be, mirroring `perf stat -e`:

- A **symbolic event name** (e.g. `INSTRUCTIONS`, `LL_READ_MISS`). The names
  known to the plugin are listed in [Symbolic event names](#symbolic-event-names) below. (**Supported**)
- A **raw PMU event** `rN`, where `N` is a hexadecimal value
  that represents the raw register encoding with the layout described by
  `/sys/bus/event_source/devices/cpu/format/*`. (**Not yet supported**)
- A a **symbolically formed PMU event**
  `pmu/config=M,config1=N,config2=K/`, where `M`, `N`, `K` are numbers (decimal, hex or octal) whose
  acceptable values are defined by `/sys/bus/event_source/devices/<pmu>/format/*`. (**Not yet supported**)
- The **named-parameter variant** `pmu/param1=0x3,param2/`,
  where `param1`/`param2` are formats defined for the PMU in
  `/sys/bus/event_source/devices/<pmu>/format/*`, and the uncore / `percore` qualifiers. (**Not yet supported**)

Any event, symbolic or raw, may be followed by an optional colon and a list of
[modifiers](#modifiers), e.g. `INSTRUCTIONS:u` or `CACHE_MISSES:u:k`.

A form that is recognised but not yet supported is rejected with an explicit "planned for a future
release" error, so the config surface stays stable: upcoming releases will encode these forms
without changing the syntax.

### Symbolic event names

Currently the plugin resolves symbolic names against the **native** kernel tables. The name is one of:

**Hardware events**:
`CPU_CYCLES`, `INSTRUCTIONS`, `CACHE_REFERENCES`, `CACHE_MISSES`, `BRANCH_INSTRUCTIONS`, `BRANCH_MISSES`, `BUS_CYCLES`, `STALLED_CYCLES_FRONTEND`, `STALLED_CYCLES_BACKEND`, `REF_CPU_CYCLES`.

**Software events**:
`PAGE_FAULTS`, `CONTEXT_SWITCHES`, `CPU_MIGRATIONS`, `PAGE_FAULTS_MIN`, `PAGE_FAULTS_MAJ`, `ALIGNMENT_FAULTS`, `EMULATION_FAULTS`, `CGROUP_SWITCHES`.

**Cache events** (`{cache-id}_{cache-op}_{cache-result}`), built from:
- `cache-id`: one of `L1D`, `L1I`, `LL`, `DTLB`, `ITLB`, `BPU`, `NODE`
- `cache-op`: one of `READ`, `WRITE`, `PREFETCH`
- `cache-result`: one of `ACCESS`, `MISS`

For example: `LL_READ_MISS`.

(Resolving arbitrary symbolic names through libpfm, for names outside these native tables, is
planned.)

To learn more about the standard events, please refer to the [`perf_event_open` manual](https://man7.org/linux/man-pages/man2/perf_event_open.2.html).
To list the events that are available on your machine, run the `perf list` command.
Note that based on your kernel version, some events could be unavailable.

### Modifiers

Modifiers restrict or refine an event; several can be chained. You can write them grouped after a
single colon (`INSTRUCTIONS:uk`) or each after its own colon (`INSTRUCTIONS:u:k`) — both are
equivalent.

For now these modifiers are supported:

| Modifier | Effect                              |
| -------- | ----------------------------------- |
| `u`      | user space only                     |
| `k`      | kernel space only                   |
| `h`      | hypervisor only                     |
| `H`      | host only (exclude guest)           |
| `G`      | guest only (exclude host)           |
| `I`      | exclude idle                        |

**Defaults.** With no modifier, an event is measured in **user space only**: kernel space and the
hypervisor are excluded. The domain modifiers (`u`/`k`/`h`) work as a group: as soon as you specify
at least one of them, the domains you *don't* list are excluded, so `:u` means "user space only" and
`:u:k` means "user and kernel, but not hypervisor".

### Metric naming

By default the metric is named `perf_{name}` (e.g. `INSTRUCTIONS` → `perf_INSTRUCTIONS`). The
modifiers do not change the metric name, so if you measure the same event with different modifiers,
give at least one of them a `rename` to avoid a name clash. A `rename` replaces the whole suffix;
the metric then becomes `perf_{rename}`.

## Configuration

Here is a configuration example of the plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.perf]
# Description.
poll_interval = "1s"
flush_interval = "1s"
events = [
    "REF_CPU_CYCLES",
    "CACHE_MISSES",
    "BRANCH_MISSES",
    "INSTRUCTIONS:u:k",                              # user-space and kernel instructions only
    { event = "CONTEXT_SWITCHES", rename = "ctxsw" }, # -> metric perf_ctxsw
    { event = "LL_READ_MISS:h", rename = "LL_READ_MISS_HYPERVISOR"}, # hypervisor instructions only
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
