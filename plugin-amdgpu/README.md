# AMD GPU plugin

Allows to measure AMD GPU hardware metrics with the ROCm software and AMD SMI library.
The new `plugin-amdgpu` currently allows you to detect AMD architecture-based GPUs installed on a machine, and collect the following metrics on each of them :

## Metrics

Here are the metrics collected by the plugin source.

|Name|Type|Unit|Description|Attributes|
|----|----|----|-----------|----------|
|`amd_gpu_clock_frequency`|u64|MegaHertz|Retrieves values ​​from AMD GPUs provided by different clocks on their compute units|Clock Type|
|`amd_gpu_energy_consumption`|u64|MilliJoule|Calculate and average between 2 measurement points based on the energy consumed since the last start-up||
|`amd_gpu_engine_usage`|u64|Percentage|Retrieves graphics units such as GFX activity (especially concerning graphic tasks)||
|`amd_gpu_fan_speed`|u64|Percentage|If it exists on the affected hardware, retrieves the GPU fan speed||
|`amd_gpu_memory_usage`|u64|MegaByte|Retrieves video compute memory (VRAM) and graphics table translation memory (GTT) usage|Memory Type|
|`amd_gpu_temperature`|u64|Celsius| Retrieves values ​​from AMD GPUs equipped with different sensors to precisely locate temperature by zone|Thermal Zone|
|`amd_gpu_pci_data_received`|u64|KiloByte per second|Retrieves the amount of data retrieved via the PCI bus||
|`amd_gpu_pci_data_sent`|u64|KiloByte per second|Retrieves the amount of data sent via the PCI bus||
|`amd_gpu_power_consumption`|u64|Watt|Calculate the estimated average electricity consumption||
|`amd_gpu_process_compute_counter`|u64|None|Retrieves the number of running computation processes||
|`amd_gpu_process_compute_unit_usage`|u64|Percentage|Retrieves the compute unit usage by a process||
|`amd_gpu_process_vram_usage`|u64|MegaByte|Retrieves VRAM used by a process||

## Requirements

To integrate the AMD GPU plugin into ALUMET, we need to use the Rust interface provided by the AMD SMI library (https://github.com/ROCm/amdsmi/tree/amd-mainline/rust-interface). However, we don't currently have a Rust crate available for compilation at https://crates.io/, unlike all the project's libraries. To compile this plugin like any other, we need to go to the `agent` directory of the ALUMET GitHub repository, then integrate and install the AMD SMI library on the machine that compiles and the machine that run ALUMET. To do this, follow the command lines below :

```bash
sudo apt-get update && sudo apt-get install -y apt-utils libdrm-dev cmake
git clone https://github.com/ROCm/amdsmi.git
mkdir amdsmi/build/ && cd amdsmi/build/
cmake .. && make -j$(nproc) && make install
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:/opt/rocm/lib
```

**WARNING** : It is important to have a recent version of `cmake`, otherwise the builder will fail to compile and describe with log messages the version you need.

## Configuration

Here is a configuration example of the AMD GPU plugin. It's part of the ALUMET configuration file (eg: `alumet-config.toml`).

```rust
[plugins.amdgpu]
poll_interval = "2s"
enable = true
```

## More information

After the installation succeed, we can compiling ALUMET to generate the usable `alumet-agent` binary file.

```bash
cargo build --release -p "alumet-agent" --bins --all-features
```

The binary has been created and is located in your ALUMET repository, in the folder `../alumet/target/release/alumet-agent`. To properly start the ALUMET program on the machine intended to collect AMD GPU metrics, simply run the `alumet-agent` binary and view the result of the collected metrics, stored by default in the `alumet-output.csv` file.

If you want to thoroughly test the AMD GPU plugin, especially to observe the usage of the GPU and its specific engines and units, you can first install `ffmpeg` and import a video into your test environment. This tool will allow you to test the GPU's video encoding and decoding processes to optimize the use of graphic effects and the overall graphics card.

```bash
sudo apt-get install ffmpeg
# Encoding a video
ffmpeg -y -vaapi_device /dev/dri/renderD128 -i <your_video.format> -vf 'format=nv12,hwupload' -c:v h264_vaapi -b:v 2M <your_output.format>
# Decoding a video
ffmpeg -y -hwaccel vaapi -vaapi_device /dev/dri/renderD128 -i <your_video.format> -vf 'format=nv12,hwupload' -c:v h264_vaapi <your_output.format>
```

If you are on the nodes of an HPC machine, it may be necessary in some cases to define specific permissions for the user group, to be able to collect all the process metrics:

```bash
sudo usermod -aG render,video $USER
```
