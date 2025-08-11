# NVIDIA NVML GPU plugin

The `plugin-nvidia-nvml` allows to detect GPUs based on NVIDIA architecture installed on a machine, measures there utilization, and retrieves various metrics about each of them.

## Requirements

To be able to run the NVML library, it is required to have previously installed NVIDIA drivers on your equipment, and have access to them:
<https://www.nvidia.com/fr-fr/drivers/unix/>

## Metrics

Here are the metrics collected by the plugin source.

|Name|Type|Unit|Description|Ressource|RessourceConsumer|Attributes|
|----|----|----|-----------|---------|-----------------|----------|
|`nvml_energy_consumption`|Counter Diff|Joule|Calculates the average between 2 measurement point base on the consumed energy since the last boot|GPU|LocalMachine||
|`nvml_instant_power`|Gauge|milliWatt|Calculates the electrical power instantly consumed|GPU|LocalMachine||
|`nvml_temperature_gpu`|Gauge|Celsius|Retrieves the main temperature emitted by a given device|GPU|LocalMachine||
|`nvml_gpu_utilization`|Gauge|Percentage|GPU rate utilization|GPU|LocalMachine||
|`nvml_encoder_sampling_period`|Gauge|microSeconds|Get the current utilization and sampling size for the decoder|GPU|LocalMachine||
|`nvml_encoder_sampling_period`|Gauge|microSeconds|Get the current utilization and sampling size for the encoder|GPU|LocalMachine||
|`nvml_n_compute_processes`|Gauge|None|Relevant currently running computing processes data|GPU|LocalMachine||
|`nvml_n_graphic_processes`|Gauge|None|Relevant currently running graphical processes data|GPU|LocalMachine||
|`nvml_memory_utilization`|Gauge|Percentage|Utilization of the GPU memory by a process|Process|LocalMachine||
|`encoder_utilization`|Gauge|Percentage|Utilization of the GPU video encoder by a process|Process|LocalMachine||
|`decoder_utilization`|Gauge|Percentage|Utilization of the GPU video decoder by a process|Process|LocalMachine||
|`sm_utilization`|Gauge|Percentage|Utilization of the GPU streaming multiprocessors by a process (3D task and rendering, etc...)|Process|LocalMachine||

## Configuration

Here is a configuration example of the plugin. It's part of the ALUMET configuration file (eg: `alumet-config.toml`).

```rust
[plugins.nvml]
poll_interval = "1s"
flush_interval = "5s"
```

## More information

To be able to see all metrics, included those concerning complex tasks like the streaming multiprocessors, and especially the video decoding and encoding activities on your GPU, you can use the `ffmpeg` software with a video file, or an NVIDIA benchmark (<https://catalog.ngc.nvidia.com/orgs/nvidia/containers/hpc-benchmarks>).
