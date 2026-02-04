# NVIDIA NVML plugin

The `nvml` plugin allows to monitor NVIDIA GPUs.

## Requirements

- Linux
- NVIDIA GPU(s)
- NVIDIA drivers installed. You probably want to use the packages provided by your Linux distribution.

## Metrics

Here are the metrics collected by the plugin's source(s).
One source will be created per GPU device.

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|---------|-----------------|----------|
|`nvml_energy_consumption`|Counter Diff|milliJoule|Average between 2 measurement points based on the consumed energy since the last boot|GPU|LocalMachine||
|`nvml_instant_power`|Gauge|milliWatt|Instant power consumption|GPU|LocalMachine||
|`nvml_temperature_gpu`|Gauge|Celsius|Main temperature emitted by a given device|GPU|LocalMachine||
|`nvml_gpu_utilization`|Gauge|Percentage (0-100)|GPU rate utilization|GPU|LocalMachine||
|`nvml_encoder_sampling_period`|Gauge|Microsecond|Current utilization and sampling size for the encoder|GPU|LocalMachine||
|`nvml_decoder_sampling_period`|Gauge|Microsecond|Current utilization and sampling size for the decoder|GPU|LocalMachine||
|`nvml_n_compute_processes`|Gauge|None|Relevant currently running computing processes data|GPU|LocalMachine||
|`nvml_n_graphic_processes`|Gauge|None|Relevant currently running graphical processes data|GPU|LocalMachine||
|`nvml_memory_utilization`|Gauge|Percentage|GPU memory utilization by a process|Process|LocalMachine||
|`nvml_encoder_utilization`|Gauge|Percentage|GPU video encoder utilization by a process|Process|LocalMachine||
|`nvml_decoder_utilization`|Gauge|Percentage|GPU video decoder utilization by a process|Process|LocalMachine||
|`nvml_sm_utilization`|Gauge|Percentage|Utilization of the GPU streaming multiprocessors by a process (3D task and rendering, etc...)|Process|LocalMachine||

Some metrics can be disabled, see the `mode` configuration option.

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.nvml]
# Initial interval between two Nvidia measurements.
poll_interval = "1s"

# Initial interval between two flushing of Nvidia measurements.
flush_interval = "5s"

# On startup, the plugin inspects the GPU devices and detect their features.
# If `skip_failed_devices = true` (or is omitted), inspection failures will be logged and the plugin will continue.
# If `skip_failed_devices = true`, the first failure will make the plugin's startup fail.
skip_failed_devices = true

# See below
mode = "full"
```

### Choosing the Right Mode

The NVML plugin offers two modes: `full` and `minimal`.

In `full` mode, all the metrics listed in the table above are provided (if they are available on the GPU).

If you want to make the GPU measurement faster, you can use the `minimal` mode.

In `minimal` mode, only `nvml_energy_consumption` and `nvml_instant_power` are provided.
The only measured value is `nvml_instant_power`. It is used to estimate `nvml_energy_consumption`.
The `minimal` mode only works on GPU that support the `nvmlDeviceGetPowerUsage` device query (the plugin detects if this is the case on startup).

## More information

Not all software use the GPU to its full extent.
For instance, to obtain non-zero values for the video encoding/decoding metrics, use a video software like `ffmpeg`.

### GPU counter updates

NVML requires 20-100ms to refresh counter values based on GPU model.
When `poll_interval` is set too low, the plugin queries identical counter values repeatedly during polling.
Since some measurements are calculated from previous polls, these measurements are discarded rather than reported as zero values.
