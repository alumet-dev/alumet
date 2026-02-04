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
```

## More information

Check more at the [user-book website](https://alumet-dev.github.io/user-book/plugins/output/prometheus.html).
