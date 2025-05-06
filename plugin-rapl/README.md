# 📄 Description

The RAPL plugin creates an Alumet **source** that collects measurements of Intel processors' energy usage via [RAPL interfaces](https://www.intel.com/content/www/us/en/developer/articles/technical/software-security-guidance/advisory-guidance/running-average-power-limit-energy-reporting.html), such as perf event and powercap.

# ⚙️ Requirements

- Linux (the plugin relies on abstractions provided by the kernel)
- [Set perf_event_paranoid to 0](https://github.com/alumet-dev/packaging/blob/main/docker/README.md#perfmon-usage) (Only for perf event usage).
- [Capabilities](https://github.com/alumet-dev/packaging/blob/main/docker/README.md) (Only for containers).

# 📊Metrics

Here are the metrics collected by the plugin source.

|name|type|unit|description|attributes|more informations|
|----|----|----|-----------|----------|-----------------|
|rapl_consumed_energy|Counter Diff|Joule|Energy consumed since the previous measurement|domain||

# 🛠️Configuration

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
