# Prometheus Exporter plugin

This crate is a library that defines the Prometheus Exporter plugin.

Implements a pull-based exporter which can be consumed by all monitoring tools compatible with the OpenMetrics specification.

## Requirements

## Configuration

Here is a configuration example of the plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.prometheus-exporter]
host = "0.0.0.0"
prefix = ""
suffix = "_alumet"
port = 9091
# Use the display name of the units instead of their unique name, as specified by the UCUM.
# See https://ucum.org/ucum for a list of unit and their symbols.
use_unit_display_name = true
add_attributes_to_labels = true
```

## More information

Check more at the [user-book website](https://alumet-dev.github.io/user-book/plugins/output/prometheus.html).
