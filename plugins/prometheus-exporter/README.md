# Prometheus Exporter plugin

This crate is a library that defines the Prometheus Exporter plugin.

Implements a pull-based exporter which can be consumed by all monitoring tools compatible with the OpenMetrics specification.

## Requirements

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.prometheus-exporter]
host = "0.0.0.0"
prefix = ""
suffix = "_alumet"
port = 9091
add_attributes_to_labels = true
# `alumet_poll_interval_ms` label with the current poll interval of the source
delta_metrics = ["rapl_consumed_energy", "nvml_energy_consumption", "amd_gpu_energy_consumption", "grace_energy_consumption", "cpu_time_delta", "kernel_cpu_time", "kernel_context_switches", "kernel_new_forks", "io_pressure_some_total", "io_pressure_full_total"]
```

## More information

Check more at the [user-book website](https://alumet-dev.github.io/user-book/plugins/output/prometheus.html).
