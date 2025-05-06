# Description

The RAPL plugin creates an Alumet **source** that collects measurements of Intel processors' energy usage via [RAPL interfaces](https://www.intel.com/content/www/us/en/developer/articles/technical/software-security-guidance/advisory-guidance/running-average-power-limit-energy-reporting.html), such as perf event and powercap.

# Requirements

- Linux (the plugin relies on abstractions provided by the kernel)

## Specificities for perf event usage

If you want to use perf event over powercap (see [should I use perf event or powercap ?](#should-i-use-perf-event-or-powercap)), make sure to follow these instructions: https://github.com/alumet-dev/packaging/blob/main/docker/README.md#perfmon-usage.

## Specificities for containers

For Alumet setup under containers, be sure to follow these instructions about RAPL: https://github.com/alumet-dev/packaging/blob/main/docker/README.md#using-rapl-plugin .

# Metrics

Here are the metrics collected by the plugin source.

|name|type|unit|description|attributes|more informations|
|----|----|----|-----------|----------|-----------------|
|rapl_consumed_energy|Counter Diff|Joule|Energy consumed since the previous measurement||

# Configuration

```toml
[plugins.rapl]
# Initial interval between two RAPL measurements.
poll_interval = "1s"
# Initial interval between two flushing of RAPL measurements.
flush_interval = "5s"
# Set to true to disable perf_events and always use the powercap sysfs.
no_perf_events = false
```


# Should I use perf event or powercap ?

blablabla
