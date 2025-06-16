# Grace-Hopper plugin

The Grace-Hopper plugin creates Alumet **sources** that collect measurements of CPU and GPU energy usage via the [ACPI power meter interface](https://docs.nvidia.com/grace-perf-tuning-guide/power-thermals.html#power-telemetry).

## Requirements

- Grace Hopper Superchip
- Linux (the plugin relies on mechanisms provided by the kernel)
- **Hwmon sysfs**: [See there to activate](#hwmon-sysfs).

## Metrics

Here is the metric collected by the plugin source.

|Name|Type|Unit|Description|Attributes|More information|
|----|----|----|-----------|----------|-----------------|
|`energy_consumed`|float|joule|Energy consumed since the previous measurement|[Sensor](#sensor)||

### Attributes

#### sensor

A sensor is a specific area of power consumption tracked by Grace-Hopper plugin.
The possible sensor values are:

|Value|Description|
|------|-----------|
|`module`|Total module power|
|`grace`|Grace Power, including DRAM and power for all regulators|
|`cpu`|CPU Power, including regulator power|
|`sysio`|SysIO Power, including regulator power|

## Configuration

Here is a configuration example of the Grace-Hopper plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.grace-hopper]
# Interval between two read of the power.
poll_interval = "1s"
# Root path to look at for hwmon file hierarchy
root_path = "/sys/class/hwmon"
```

## More information

### Source

All information in this README comes from:
- [power-thermals](https://docs.nvidia.com/grace-perf-tuning-guide/power-thermals.html#power-telemetry)
- [grace patch config guide](https://docs.nvidia.com/grace-patch-config-guide.pdf)
- Tested chip: The Grace Hopper Superchip

### hwmon-sysfs

To enable and view `hwmon` sysfs nodes, ensure the following configuration:

1. Kernel Configuration
Set the following option in your kernel configuration (`kconfig`):
    > CONFIG_SENSORS_ACPI_POWER=m

2. Kernel Command Line Parameter
Add the following parameter to your kernel command line:
    > acpi_power_meter.force_cap_on=y

These settings ensure that the ACPI power meter driver is available and exposes the necessary `hwmon` interfaces.

You could see your current kernel configuration about the ACPI POWER sensor using:
- `zcat /proc/config.gz | grep CONFIG_SENSORS_ACPI_POWER`
- `grep CONFIG_SENSORS_ACPI_POWER /boot/config-$(uname -r)`
- `modinfo acpi_power_meter`

More information could be found [on the grace patch configuration guide](https://docs.nvidia.com/grace-patch-config-guide.pdf)
