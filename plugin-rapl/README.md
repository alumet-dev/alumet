# 📄 Description

The RAPL plugin creates an Alumet **source** that collects measurements of Intel processors' energy usage via [RAPL interfaces](https://www.intel.com/content/www/us/en/developer/articles/technical/software-security-guidance/advisory-guidance/running-average-power-limit-energy-reporting.html), such as perf-events and powercap.

# ⚙️ Requirements

- Linux (the plugin relies on abstractions provided by the kernel)
- [Set perf_event_paranoid to 0](https://github.com/alumet-dev/packaging/blob/main/docker/README.md#perfmon-usage) (Only for perf-events usage).
- [Capabilities](https://github.com/alumet-dev/packaging/blob/main/docker/README.md) (Only for containers).

# 📊Metrics

Here are the metrics collected by the plugin source.

|Name|Type|Unit|Description|Attributes|More informations|
|----|----|----|-----------|----------|-----------------|
|rapl_consumed_energy|Counter Diff|Joule|Energy consumed since the previous measurement|domain||

## Domain attribute

A consumed energy measure is classified by the "domain" attribute, here are the possible values:

|Domain|Description|
|------|-----------|
|platform|the entire server|
|package|the CPU cores, the iGPU, the L3 cache and the controllers|
|pp0|the CPU cores|
|pp1|the iGPU|
|dram|the RAM attached to the processor|

# 🛠️Configuration

```toml
[plugins.rapl]
# Interval between two RAPL measurements.
poll_interval = "1s"
# Interval between two flushing of RAPL measurements.
flush_interval = "5s"
# Set to true to disable perf_events and always use the powercap sysfs.
no_perf_events = false
```

# Should I use perf-events or powercap ?

While using perf-events or powercap will give you similar results, it's recommended to use perf-events to limit the measurement overhead.
Note that perf-events requires some capabalities while powercap don't.

If you want deep details about the difference between both you can see [this publication that deep dive into RAPL measurements](https://hal.science/hal-04420527v2/document).
