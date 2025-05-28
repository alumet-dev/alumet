# RAPL plugin

The RAPL plugin creates an Alumet **source** that collects measurements of processor energy usage via [RAPL interfaces](https://www.intel.com/content/www/us/en/developer/articles/technical/software-security-guidance/advisory-guidance/running-average-power-limit-energy-reporting.html), such as perf-events and powercap.

## Requirements

- RAPL-compatible processor
- Linux (the plugin relies on abstractions provided by the kernel - perf-events and powercap)
- **Specific for perf-events usage**: [See perf_event_paranoid and capabilities requirements](#perf_event_paranoid-and-capabilities).
- **Specific for powercap usage**: Ensure read access to everything in `/sys/devices/virtual/powercap/intel-rapl` (eg: `sudo chmod a+r -R /sys/devices/virtual/powercap/intel-rapl`).
- **Specific for containers**: Read [this documentation about rapl plugin capabilities](https://github.com/alumet-dev/packaging/blob/main/docker/README.md#using-rapl-plugin).

## Metrics

Here are the metrics collected by the plugin source.

|Name|Type|Unit|Description|Attributes|More information|
|----|----|----|-----------|----------|-----------------|
|`rapl_consumed_energy`|Counter Diff|joule|Energy consumed since the previous measurement|[domain](#domain)||

### Attributes

#### Domain

A domain is a specific area of power consumption tracked by RAPL.
The possible domain values are:

|Value|Description|
|------|-----------|
|`platform`|the entire machine - ⚠️ may vary depending on the model|
|`package`|the CPU cores, the iGPU, the L3 cache and the controllers|
|`pp0`|the CPU cores|
|`pp1`|the iGPU|
|`dram`|the RAM attached to the processor|

## Configuration

Here is a configuration example of the RAPL plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.rapl]
# Interval between two RAPL measurements.
poll_interval = "1s"
# Interval between two flushing of RAPL measurements.
flush_interval = "5s"
# Set to true to disable perf-events and always use the powercap sysfs.
no_perf_events = false
```

## More information

### Should I use perf-events or powercap ?

Both interfaces provide similar energy consumption data, but we recommend using perf-events for lower measurement overhead (especially in high-frequency polling scenarios).

For a more detailed technical comparison, see [this publication on RAPL measurement methods](https://hal.science/hal-04420527v2/document).

### perf_event_paranoid and capabilities

You should read this section **in case you're using perf-events** to collect measurements.

`perf_event_paranoid` is a Linux kernel setting that controls the level of access that unprivileged (non-root) users have to access features provided by the `perf` subsystem which can be used in this plugin ([should I use perf-events or powercap](#should-i-use-perf-events-or-powercap-)).

Below is a summary of how different perf_event_paranoid values affect RAPL plugin functionality when running as an unprivileged user:

| `perf_event_paranoid` value     | Description                                            | Required capabilities (binary)                       | RAPL plugin works (unprivileged) |
| ------------------------------- | ------------------------------------------------------ | ---------------------------------------------------- | -------------------------------- |
| 4 *(Debian-based systems only)* | Disables all perf event usage for unprivileged users   | –                                                    | ❌ Not supported                 |
| 2                               | Allows only user-space measurements                    | `cap_perfmon` *(or `cap_sys_admin` for Linux < 5.8)* | ✅ Supported                     |
| 1                               | Allows user-space and kernel-space measurements        | `cap_perfmon` *(or `cap_sys_admin` for Linux < 5.8)* | ✅ Supported                     |
| 0                               | Allows user-space, kernel-space, and CPU-specific data | `cap_perfmon` *(or `cap_sys_admin` for Linux < 5.8)* | ✅ Supported                     |
| -1                              | Full access, including raw tracepoints                 | –                                                    | ✅ Supported                     |

Example for setting `perf_event_paranoid`: `sudo sysctl -w kernel.perf_event_paranoid=2` will set the value to **2**.

Note that this command will not make it permanent (reset after restart).

Alternatively, you can run Alumet as a **privileged user** (root), but this is not recommended for security reasons.
