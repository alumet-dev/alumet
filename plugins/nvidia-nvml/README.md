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
|`nvml_energy_consumption`|Counter Diff|Joule|Average between 2 measurement points based on the consumed energy since the last boot|GPU|LocalMachine||
|`nvml_instant_power`|Gauge|Milliwatt|Instant power consumption|GPU|LocalMachine||
|`nvml_temperature_gpu`|Gauge|Celsius|Main temperature emitted by a given device|GPU|LocalMachine||
|`nvml_gpu_utilization`|Gauge|Percentage|GPU rate utilization|GPU|LocalMachine||
|`nvml_encoder_sampling_period`|Gauge|Microsecond|Current utilization and sampling size for the encoder|GPU|LocalMachine||
|`nvml_decoder_sampling_period`|Gauge|Microsecond|Current utilization and sampling size for the decoder|GPU|LocalMachine||
|`nvml_n_compute_processes`|Gauge|None|Relevant currently running computing processes data|GPU|LocalMachine||
|`nvml_n_graphic_processes`|Gauge|None|Relevant currently running graphical processes data|GPU|LocalMachine||
|`nvml_memory_utilization`|Gauge|Percentage|GPU memory utilization by a process|Process|LocalMachine||
|`nvml_encoder_utilization`|Gauge|Percentage|GPU video encoder utilization by a process|Process|LocalMachine||
|`nvml_decoder_utilization`|Gauge|Percentage|GPU video decoder utilization by a process|Process|LocalMachine||
|`nvml_sm_utilization`|Gauge|Percentage|Utilization of the GPU streaming multiprocessors by a process (3D task and rendering, etc...)|Process|LocalMachine||

## Configuration

Here is a configuration example of the plugin. It's part of the ALUMET configuration file (eg: `alumet-config.toml`).

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
```

## More information

Not all software use the GPU to its full extent.
For instance, to obtain non-zero values for the video encoding/decoding metrics, use a video software like `ffmpeg`.
