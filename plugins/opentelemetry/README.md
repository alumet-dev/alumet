# OpenTelemetry plugin

This crate is a library that defines the OpenTelemetry plugin.

Implements a push-based exporter (via gRPC) which can be connected to an OpenTelemetry Collector (via a receiver), processed in any way, and then exported to a observability backend like Jaeger, Prometheus, Thanos, OpenSearch, ElasticSearch, etc.

## Requirements

- It needs to have a OTEL Collector reachable.

## Configuration

Here is a configuration example of the plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.opentelemetry]
# Behaviour configuration 
collector_host = "http://localhost:4317"
push_interval_seconds = 15
# Metric's name configuration
prefix = ""
suffix = "_alumet"
# Use the display name of the units instead of their unique name, as specified by the UCUM.
# See https://ucum.org/ucum for a list of unit and their symbols.
use_unit_display_name = true
add_attributes_to_labels = true
```

## More information

Check more at the [user-book website](https://alumet-dev.github.io/user-book/plugins/output/opentelemetry.html).
