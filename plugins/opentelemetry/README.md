# OpenTelemetry plugin

This crate is a library that defines the OpenTelemetry plugin.

Implements a push-based exporter (via OTLP/gRPC) which can be connected to an OpenTelemetry Collector (via a receiver), processed in any way, and then exported to an observability backend like Jaeger, Prometheus, Thanos, OpenSearch, ElasticSearch, etc.

Each call to `write()` triggers an immediate gRPC export, allowing metrics to be exported at any frequency.

## Requirements

- A reachable OpenTelemetry Collector with an OTLP/gRPC receiver enabled.

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.opentelemetry]
# URL of the OTLP gRPC collector
collector_host = "http://localhost:4317"
# Optional prefix and suffix applied to every exported metric name
prefix = ""
suffix = "_alumet"
# Use the display name of the units instead of their unique name, as specified by the UCUM.
# See https://ucum.org/ucum for a list of units and their symbols.
# Example: "W" (display) vs "watt" (unique)
use_unit_display_name = true
# Forward measurement attributes as OpenTelemetry data-point attributes
add_attributes_to_labels = true
```

## More information

Check more at the [user-book website](https://alumet-dev.github.io/user-book/plugins/output/opentelemetry.html).

> **Note**: If a point with an empty attribute value is received, this plugin will set it's value to `"empty"` before forwarding it to OpenTelemetry.

### How does the push frequency interact with other Alumet plugins?

Being a push-based exporter, the **frequency at which the Alumet/OTEL plugin sends requests is determined solely by the `flush_interval` of your sources**.

#### Example 1: Simple RAPL source

```toml
[plugins.rapl]
# Interval between two RAPL measurements.
poll_interval = "5ms"
# Interval between two flushing of RAPL measurements.
flush_interval = "2s"
# Set to true to disable perf-events and always use the powercap sysfs.
no_perf_events = false

# ...
```

With this configuration, RAPL takes a measurement every 5ms, but the resulting points are batched and flushed downstream every 2s. This yields (2s / 0.005s) × 6 = **2400 points per flush**.

> **Note**: The ×6 factor comes from RAPL producing 6 distinct points per measurement: `package`, `pp0`, `platform`, `package_total`, `pp0_total`, and `platform_total`.

Each flush delivers a single `MeasurementBuffer` to the Alumet/OTEL plugin, which translates it into one OTLP/gRPC request carrying all 2400 points — one request every 2s.

The resulting traffic seen by the OTEL Collector looks like:

```c
time: 12:00 -> RAPL    ->  2400 points
time: 12:02 -> RAPL    ->  2400 points
time: 12:04 -> RAPL    ->  2400 points
...
```

#### Example 2: RAPL + procfs (with different `flush_interval` values)

Now consider running two sources with different flush intervals:

```toml
[plugins.rapl]
# Interval between two RAPL measurements.
poll_interval = "5ms"
# Interval between two flushing of RAPL measurements.
flush_interval = "2s"
# Set to true to disable perf-events and always use the powercap sysfs.
no_perf_events = false

[plugins.procfs.processes.events]
# Interval between two measurements for event-driven process monitoring.
poll_interval = "500ms"
# How frequently should the processes information be flushed to the rest of the pipeline.
flush_interval = "4s"
# Which method to use to obtain memory statistics.
memory_mode = "quick"

# ...
```

RAPL continues to flush 2400 points every 2s as before. **In parallel**, procfs flushes (4s / 0.500s) × 9 = **72 points every 4s**.

> **Note**: Note: Unlike RAPL, procfs creates one source per watched process. The number of concurrent batches therefore depends on how many processes Alumet is monitoring. With alumet exec (a single process), there is just one procfs source — but in typical watch mode, many processes are tracked simultaneously, each flushing their own MeasurementBuffer independently.

Each source flushes independently: the Alumet/OTEL plugin receives a separate `MeasurementBuffer` from each and issues a dedicated OTLP/gRPC request for each — concurrently. The resulting traffic diagram below reflects the single-process case; with multiple watched processes, several additional concurrent procfs requests would appear at each 4s mark.

```c
time: 12:00 -> RAPL    ->  2400 points
time: 12:02 -> RAPL    ->  2400 points
time: 12:04 -> procfs  ->    72 points
time: 12:04 -> RAPL    ->  2400 points
time: 12:06 -> RAPL    ->  2400 points
time: 12:08 -> procfs  ->    72 points
time: 12:08 -> RAPL    ->  2400 points
time: 12:10 -> RAPL    ->  2400 points
time: 12:12 -> procfs  ->    72 points
...
```
