# NVIDIA NVML GPU plugin

The `plugin-nvidia-nvml` allows to detect GPUs based on NVIDIA architecture installed on a machine, measures them utilization, and retrieves various metrics about each of them.

## Metrics

Here are the metrics collected by the plugin source.

|Name|Type|Unit|Description|
|----|----|----|-----------|
|`total_energy_consumption`|Counter Diff|MilliJoule|Calculates the average between 2 measurement point base on the consumed energy since the last boot|
|`instant_power`|u64|Watt|Calculates the electrical power instantly consumed|
|`temperature_gpu`|u64|Celsius|Retrieves the main temperature emitted by a given device.|
|`major_utilization_gpu`|u64|Percentage|GPU rate utilization|
|`major_utilization_memory`|u64|Percentage|GPU memory utilization|
|`decoder_utilization`|u64|Percentage|GPU video decoding property|
|`decoder_sampling_period_us`|u64|microSeconds|Get the current utilization and sampling size for the decoder GPU unit|
|`encoder_utilization`|u64|Percentage|GPU video encoding property|
|`encoder_sampling_period_us`|u64|microSeconds|Get the current utilization and sampling size for the encoder GPU unit|
|`sm_utilization`|u64|Percentage|Time consumed by the streaming multiprocessors of a GPU (3D task and rendering, etc...)|
|`running_compute_processes`|u64|Percentage|Relevant currently running computing processes data|
|`running_graphics_processes`|u64|Percentage|Relevant currently running graphical processes data|

## Configuration

Here is a configuration example of the NVML NVIDIA plugin. It's part of the ALUMET configuration file (eg: `alumet-config.toml`).

```rust
[plugins.nvml]
poll_interval = "1s"
flush_interval = "5s"
```

## More information

We can compiling ALUMET to generate the usable **alumet-agent** binary file, without other requirements that the `nvml` Rust crate.

```bash
cd alumet/agent/
cargo build --release -p "alumet-agent" --bins --all-features
```

The binary was finally created and is located in your ALUMET repository in "../target/release/alumet-agent" folder. To start correctly ALUMET program on the machine intended to collect NVIDIA GPU metrics, we need to install nvidia-driver on system, and just run the binary `alumet-agent`.

Also, to be able to see the parameters of complex tasks like the streaming multiprocessors, and especially the video decoding and encoding activities on your GPU, you can use the `ffmpeg` software with a video file, or an NVIDIA benchmark (<https://catalog.ngc.nvidia.com/orgs/nvidia/containers/hpc-benchmarks>).
