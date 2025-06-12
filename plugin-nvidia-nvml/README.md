# NVML plugin for NVIDIA GPUs

The `nvidia-nvml` plugin allows to measure the utilization of a dedicated NVIDIA GPU.

## Table of Contents

- [Description](#description)
- [Configuration](#configuration)
- [Use](#use)

### Description

The `plugin-nvidia-nvml` currently allows to detect GPUs based on NVIDIA architecture installed on a machine, and collect the following metrics about each of them :

> - **Energy consumption** : Calculates and average between 2 measurement point base on the consumed energy since the last boot.
> - **Power consumption** : Calculates the electrical power instantly consumed.
> - **Memory used** : Retrieves the current memory's utilization rates for a device.
> - **Encoding / Decoding** : Video encoding and decoding unit utilization.
> - **Thermal zone** : Retrieves the main temperature emitted by a given device.
> - **Process** : Get the number of computing and graphic process, and retrieves their associated information :
>   - **Streaming Multiprocessor** : Refers to the percentage of time that the Streaming Multiprocessors of a GPU.
>   - **Memory** : Frame buffer memory utilization.
>   - **Encoding / Decoding** : Video encoding and decoding unit utilization.

### Configuration

In the **alumet-config.toml** used by ALUMET main program as configuration file, you can modify the parameters of this section to set the activation or deactivation of the plugin, and the time interval to collect metrics.

```rust
[plugins.nvml]
poll_interval = "1s"
flush_interval = "5s"
```

### Use

We can compiling ALUMET to generate the usable **alumet-agent** binary file, without other requirements that the `nvml` Rust crate.

```bash
cd alumet/agent/
cargo build --release -p "alumet-agent" --bins --all-features
```

The binary was finally created and is located in your ALUMET repository in "../target/release/alumet-agent" folder. To start correctly ALUMET program on the machine intended to collect NVIDIA GPU metrics, we need to install nvidia-driver on system, and just run the binary `alumet-agent`.
