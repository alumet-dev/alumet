# Filter Plugin

The Filter plugin creates an Alumet **transform** that can filter in/out measurements downstream based on metric names.

## Configuration

Here is two configuration examples of the Filter Plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.filter]
# The metrics names you want to filter IN downstream
# Here only the measurements with metric name `cpu_time_delta` will be kept in the measurement buffer
include = ["cpu_time_delta"]
```

```toml
[plugins.filter]
# The metrics names you want
# Here all the measurements with metric name `cpu_time_delta` will be removed from the measurement buffer
exclude = ["cpu_time_delta"]
```

Note that you need to choose between either `include` or `exclude` parameter.
An error occurs when you define both.
