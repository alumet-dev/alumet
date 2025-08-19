# Jetson plugin

The `jetson` plugin allows to measure the power consumption of Jetson edge devices by querying their internal INA-3221 sensor(s).

## Requirements

This plugin only works on NVIDIA Jetson™ devices.
It supports Jetson Linux versions 32 to 36 (JetPack 4.6 to 6.x), and will probably work fine with future versions.

The plugin needs to read files from the sysfs, so it needs to have the permission to read the I2C hierarchy of the INA-3221 sensor(s).
Depending on your system, the root of this hierarchy is located at:
- `/sys/bus/i2c/drivers/ina3221` on modern systems,
- `/sys/bus/i2c/drivers/ina3221x` on older systems

## Metrics

The plugin source can collect the following metrics.
Depending on the hardware, some metrics may or may not be collected.

|Name|Type|Unit|Description|Attributes|
|----|----|----|-----------|----------|
|`input_current`| u64 | mA (milli-Ampere) | current intensity on the channel's line | see below |
|`input_voltage`| u64 | mV (milli-Volt)| current voltage on the channel's line    | see below |
|`input_power`  | u64 | mW (milli-Watt)| instantaneous electrical power on the channel's line    | see below |

### Attributes

The sensor provides measurements for several **channels**, which are connected to different parts of the hardware (this depends on the exact model of the device). This is reflected in the attributes attached to the measurement points.

Each measurement point produced by the plugin has the following attributes:
- `ina_device_number` (u64): the sensor's device number
- `ina_i2c_address` (u64): the I2C address of the sensor
- `ina_channel_id` (u64): the identifier of the channel
- `ina_channel_label` (str): the label of the channel

Refer to the documentation of your Jetson to learn more about the channels that are available on your device.

### Example

On the Jetson Xavier NX Developer Kit, one sensor is connected to the I2C sysfs, at `/sys/bus/i2c/drivers/ina3221/7-0040/hwmon/hwmon6`. It features 4 channels:
- Channel 1: `VDD_IN`
  - Files `in1_label`, `curr1_input`, etc.
- Channel 2: `VDD_CPU_GPU_CV`
  - Files `in2_label`, `in2_input`, etc.
- Channel 3: `VDD_SOC`
  - Files `in2_label`, `in2_input`, etc.
- Channel 7: `sum of shunt voltages`
  - Files `in7_label`, `in7_input`, etc.

When measuring the data from _channel 1_, the plugin will produce measurements with the following attributes:
- `ina_device_number: 6`
- `ina_i2c_address: 0x40` (64 in decimal)
- `ina_channel_id: 1`
- `ina_channel_label: "VDD_IN"`

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.jetson]
poll_interval = "1s"
flush_interval = "5s"
```

## More information

To find the model of your Jetson, run:

```sh
cat /sys/firmware/devicetree/base/model
```
