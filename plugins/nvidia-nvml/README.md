# NVIDIA NVML plugin

The `plugin-nvidia-nvml` allows to detect GPUs based on NVIDIA architecture installed on a machine, measures their utilization and retrieves various metrics about each of them.

## Requirements

- Linux
- NVIDIA GPU
- NVIDIA drivers installed, see the [Unix Driver Archive](https://www.nvidia.com/en-us/drivers/unix/) to install them

## Metrics

Here are the metrics collected by the plugin source.

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
```

## More information

To be able to see all metrics, included those concerning complex tasks like the streaming multiprocessors, and especially the video decoding and encoding activities on your GPU, you can use the `ffmpeg` software with a video file or an [NVIDIA benchmark](https://catalog.ngc.nvidia.com/orgs/nvidia/containers/hpc-benchmarks).
