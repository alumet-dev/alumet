# lm-sensors plugin

This pugin creates an Alumet **autonomous source** that collects measurements from [lm-sensors](https://github.com/lm-sensors/lm-sensors) and based on [this crate](https://codeberg.org/koutheir/lm-sensors.git).


## Requirements

- Linux (lm-sensors relies on hardware monitoring support from Linux)
- For temperature measures an Intel processor is required (**for the moment** only Intel processors are supported because the plugin retrieves information from the [coretemp kernel driver](https://docs.kernel.org/hwmon/coretemp.html))

## Metrics

Here are the metrics collected by the plugin source.

### Temperature

For the moment this is the only metric supported by this plugin.
lm-sensors collects the temperature of CPU packages and CPU cores in Celsius degrees.
Despite the unit being float, lm-sensors seem to report temperature values rounded to integer.

|Name|Type|Unit|Description|Resource|More information|
|----|----|----|-----------|--------|----------------|
|`lm_sensors_temperature`|float|Celsius|Temperature measured by the sensor|[resources](#resources)||

#### Resources

Temperature sensors target either a CPU package or a CPU core.

| `resource_kind` field | Example values of the `resource_id` field |
|----|----|
| `cpu_package` | `0` or `1` for two CPU packages  |
| `cpu_core`    | `0_0` for CPU core 0 of Package 0, `1_12` for for 12 of package `1` |


## Configuration

Here is an example of how to configure the lm-sensors plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.lm-sensors]
# Interval between two measurements (default value 1s).
poll_interval = "1s"
# Interval between two flushing of measurements (default value 5s).
flush_interval = "5s"
# To enable temperature measurements (default value false).
enable_temperature = true
# Temperature-only configuration (default value false).
# Enable to get measurements from the CPU packages only.
coretemp_package_only = false
```

Note that all configuration fields of this plugin are optional and take default values if the field is missing in the Alumet configuration file.
