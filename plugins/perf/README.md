# Perf plugin

The perf plugin creates an Alumet **source** that collects measurements using the Performance Counters for Linux (aka `perf_events`).
It can obtain valuable data about the system and/or a specific process, such as the number of instructions executed, cache-misses suffered, …
This plugin works in a similar way to the [`perf` command-line tool](https://man7.org/linux/man-pages/man1/perf.1.html).

## Requirements

- Linux (`perf_events` is a kernel feature)
- [Required capabilities](#perf_event_paranoid-and-capabilities).
- [libpfm4](#libpfm-events). **Optional**. Only required to read libpfm-encoded events.

## Events

Events to measure are described with a single unified syntax. The **event name** have a syntax inspired from
`perf stat -e` [event selection syntax](https://man7.org/linux/man-pages/man1/perf-stat.1.html).

Each event is a string of the form:

```
<event>[#<modifiers>]
```

### Event formats

The `<event>` part can be one of five forms:

- **native** : a symbolic event name (e.g. `INSTRUCTIONS`, `LL_READ_MISS`), resolved against the
  built-in kernel tables listed in [Native event names](#native-event-names). (**Supported**)
- **libpfm** : any other name, optionally with unit masks (e.g. `RESOURCE_STALLS:ANY`,
  `MEM_LOAD_RETIRED:L3_MISS`), resolved through [libpfm](#libpfm-events). (**Supported**, requires libpfm)
- **raw-hex** : a raw code `rN`, where `N` is a hexadecimal value representing the raw register
  encoding, with the layout described by `/sys/bus/event_source/devices/cpu/format/*`. (**Not yet supported**)
- **pmu-named** : `pmu/event=M,umask=N,…/`, using named fields defined in
  `/sys/bus/event_source/devices/<pmu>/format/*` (perf packs the bits for you); also the uncore /
  `percore` qualifiers. (**Not yet supported**)
- **pmu-raw** : `pmu/config=M,config1=N,config2=K/`, giving the raw config registers directly
  (numbers in decimal, hex or octal). (**Not yet supported**)

Any event may be followed by `#` and a list of [modifiers](#modifiers), e.g.
`INSTRUCTIONS#u` or `CACHE_MISSES#u:k`.

### Native event names

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

To learn more about the standard events, please refer to the [`perf_event_open` manual](https://man7.org/linux/man-pages/man2/perf_event_open.2.html).
To list the events that are available on your machine, run the `perf list` command.
Note that based on your kernel version, some events could be unavailable.

### Libpfm events

The native tables above are a small, vendor-neutral subset. Any name they don't recognise is passed
to [libpfm4](https://perfmon2.sourceforge.net/), which knows the hundreds of microarchitecture-specific
PMU events and their unit masks (e.g. `RESOURCE_STALLS:ANY`). List what your CPU exposes with
libpfm's `showevtinfo` tool.

A libpfm event name is structured like this (parsing is case-insensitive):

`[pmu::]event_name[:unit_mask][:unit_mask…]`

- **`pmu::`** *(optional)* : a libpfm PMU / microarchitecture model, e.g. `ix86arch::`, to
  disambiguate an event. Usually unnecessary : libpfm auto-detects your CPU.
- **`event_name`** *(required)* : the full event name, e.g. `RESOURCE_STALLS`.
- **`:unit_mask`** *(optional, repeatable)* : a sub-event that refines the event, e.g. `:ANY` or
  `:L3_MISS`. Some events require one; some accept several.

**Note:** In native libpfm, privilege modifiers are written on the event itself (e.g. `:u:k`).
This plugin does not interpret them that way: anything after a : is passed to libpfm as a unit mask, and
privilege/domain modifiers are expressed uniformly for all event formats with the `#u:k` syntax instead.
See the [modifiers section](#modifiers) for more details.

libpfm is **loaded at runtime** (via `dlopen`), not linked at build time:

- It is only needed if your configuration uses libpfm-encoded events.
- If it is missing, any libpfm-encoded event in your configuration makes startup fail with a clear
  error . A configuration with no libpfm event starts fine.
- Install it from your distribution (e.g. `libpfm4` on Debian/Ubuntu). By default the plugin looks
  for `libpfm.so.4` and `libpfm.so`. Set `ALUMET_LIBPFM_LIB` to a `.so` name or full path to override.

### Modifiers

Modifiers are attached after a `#`, one per `:`-separated token (e.g. `INSTRUCTIONS#u:k`). Each
An unknown token (e.g. `INSTRUCTIONS#z`) is rejected.

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
at least one of them, the domains you *don't* list are excluded, so `#u` means "user space only" and
`#u:k` means "user and kernel, but not hypervisor".

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
    "INSTRUCTIONS#u:k",                              # user-space and kernel instructions only
    "RESOURCE_STALLS:ANY",                           # libpfm event with a unit mask
    { event = "CONTEXT_SWITCHES", rename = "ctxsw" }, # -> metric perf_ctxsw
    { event = "LL_READ_MISS#h", rename = "LL_READ_MISS_HYPERVISOR"}, # hypervisor only
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
