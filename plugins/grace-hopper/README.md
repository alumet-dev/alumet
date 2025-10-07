# Grace Hopper plugin

The `grace-hopper` plugin collect measurements of CPU and GPU energy usage of NVIDIA **Grace** and **Grace Hopper** superchips.

## Requirements

- Grace or Grace Hopper superchip
- [Grace hwmon sensors enabled](#hwmon-sysfs)

## Metrics

Here are the metrics collected by the plugin.

|Name|Type|Unit|Description|Attributes|More information|
|----|----|----|-----------|----------|-----------------|
|`grace_instant_power`|uint|microWatt|Power consumption|[sensor](#hardware-sensors)| If the `resource_kind` is `LocalMachine` then the value is the sum of all sensors of the same type|
|`grace_energy_consumption`|float|milliJoule|Energy consumed since the previous measurement|[Sensor](#hardware-sensors)| If the `resource_kind` is `LocalMachine` then the value is the sum of all sensors of the same type |

The hardware sensors do not provide the energy, only the power.
The plugin computes the energy consumption with a discrete integral on the power values.

### Attributes

#### Hardware Sensors

The Grace and Grace Hopper superchips track the power consumption of [several areas](https://docs.nvidia.com/grace-perf-tuning-guide/power-thermals.html#fig-grace-power-telemetry-sensors).
The area is indicated by the `sensor` attribute of the measurements points.

The base possible values are:

|`sensor` value|Description|Grace|Grace Hopper|
|-----|-----------|-----|------------|
|`module`|Total power of the Grace Hopper module, including regulator loss and DRAM, GPU and HBM power.|No|Yes|
|`grace`|Power of the Grace socket (the socket number is indicated by the point's resource id)|Yes|Yes|
|`cpu`|CPU rail power|Yes|Yes|
|`sysio`|SOC rail power|Yes|Yes|

Refer to the next section for more values.

#### Sums and Estimations

The `grace-hopper` plugins computes additional values and tag them with a different `sensor` value, according to the table below.

|`sensor` value|Description|
|-----|-----------|
|`dram`|Estimated power or energy consumption of the DRAM (memory)|
|`module_total`|sum of all `module` values for the corresponding metric|
|`grace_total`|sum of all `grace` values|
|`cpu_total`  |sum of all `cpu` values|
|`sysio_total`|sum of all `sysio` values|
|`dram_total`|sum of all `dram` values|

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

### hwmon sysfs

This plugin reads the power telemetry data provided via `hwmon`.
To enable the `hwmon` virtual devices for Grace/GraceHopper, configure your system as follows:

1. Kernel Configuration
Set the following option in your kernel configuration (`kconfig`):
    > CONFIG_SENSORS_ACPI_POWER=m

1. Kernel Command Line Parameter
Add the following parameter to your kernel command line:
    > acpi_power_meter.force_cap_on=y

These settings ensure that the ACPI power meter driver is available and exposes the necessary `hwmon` interfaces.

You could see your current kernel configuration about the ACPI POWER sensor using:
- `zcat /proc/config.gz | grep CONFIG_SENSORS_ACPI_POWER`
- `grep CONFIG_SENSORS_ACPI_POWER /boot/config-$(uname -r)`
- `modinfo acpi_power_meter`

More information can be found [on the NVIDIA Grace Platform Configurations Guide](https://docs.nvidia.com/grace-patch-config-guide.pdf).
