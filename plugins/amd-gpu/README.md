# AMD GPU plugin

Allows to measure AMD GPU hardware metrics with the ROCm software and `amdsmi` library.
The new `plugin-amdgpu` currently allows you to detect AMD architecture-based GPUs installed on a machine, and collect the following metrics on each of them :

## Requirements

- Linux operating system
- Installation of the graphic library [libdrm](https://github.com/Distrotech/libdrm).
- Integrate in ALUMET compilation the [Rust interface](https://github.com/ROCm/amdsmi/tree/amd-mainline/rust-interface) provided by the `amdsmi` library. In deed, we don't currently have a Rust crate available for compilation at https://crates.io/, unlike all the project's libraries.
- Build manually the `amdsmi` with [cmake](https://cmake.org/) DEB package based distro. Generally, RPM package based distro have already in their repository the `amdsmi` installable as classic package. It is important to have a recent version of cmake, otherwise the builder will fail to compile and describe with log messages the version you need.

```bash
sudo apt-get update && sudo apt-get install -y apt-utils libdrm-dev cmake
git clone https://github.com/ROCm/amdsmi.git
mkdir amdsmi/build/ && cd amdsmi/build/
cmake .. && make -j$(nproc) && make install
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:/opt/rocm/lib
```

## Metrics

Here are the metrics collected by the plugin source:

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`amd_gpu_energy_consumption`|CounterDiff|MilliJoule|Calculate and average between 2 measurement points based on the energy consumed since the last start-up|GPU|LocalMachine||
|`amd_gpu_engine_usage`|Gauge|percentage|Retrieves graphics units such as GFX activity (especially concerning graphic tasks)|GPU|LocalMachine||
|`amd_gpu_memory_usage`|Gauge|megabyte|Retrieves video compute memory (VRAM) and graphics table translation memory (GTT) usage|GPU|LocalMachine|[memory_type](#memory_type)|
|`amd_gpu_temperature`|Gauge|celsius| Retrieves values ​​from AMD GPUs equipped with different sensors to precisely locate temperature by zone|GPU|LocalMachine|[thermal_zone](#thermal_zone)|
|`amd_gpu_power_consumption`|Gauge|watt|Calculate the estimated average electricity consumption|GPU|LocalMachine||
|`amd_gpu_process_compute_counter`|Gauge|none|Retrieves the number of running computation processes|GPU|LocalMachine||
|`amd_gpu_process_compute_unit_usage`|Gauge|percentage|Retrieves the compute unit usage by a process|GPU|LocalMachine||
|`amd_gpu_process_vram_usage`|Gauge|megabyte|Retrieves VRAM used by a process|GPU|LocalMachine||

### Attributes

#### memory_type

The memory type defines the type of consumed memory by GPU :

|Value|Description|
|-----|-----------|
|`memory_graphic_translation_table`|The GTT memory is a portion of system memory that can be used as a virtual extension when dedicated VRAM is insufficient, or in addition to it, and allowing to increase available graphics memory but with higher latency|
|`memory_video_computing`|The memory video computing is a fast memory integrated on the graphics card, storing graphics data for efficient rendering|

#### thermal_zone

The architecture of AMD GPUs is broken down into several type zones associated with a thermal sensor, to analyse precisely the GPU hardware temperature :

|Value|Description|
|-----|-----------|
|`thermal_global`|The global temperature measured on the AMD GPU hardware|
|`thermal_hotspot`|Value measured by a probe able to locating the maximal temperature on the AMD GPU hardware|
|`thermal_vram`|Temperature only emitted by video computing memory of the AMD GPU hardware|
|`thermal_high_bandwidth_memory_X`|Temperature measured on GPU equipped with High Bandwidth Memory, designed to deliver high data transfer while minimizing power consumption in same time. Each "X" index (0 to 3) corresponding to a specific HBM stack|
|`thermal_pci_bus`|Temperature concerning only the PCI Express data BUS (PCIe), the interface between GPU and others components|

## Configuration

Here is a configuration example of the plugin. It's part of the ALUMET configuration file (eg: `alumet-config.toml`).

```toml
[plugins.amdgpu]
poll_interval = "2s"
```

## More information

**IMPORTANT** : Although this plugin works by returning metrics, it remains experimental and the functions and values returned by the `amdsmi` library change regularly. There are some bugs in the values and some functions are not working, and are being fixed by the `amdsmi` and ROCm development team.

If you want to thoroughly test the AMD GPU plugin, especially to observe the usage of the GPU and its specific engines and units, you can first install `ffmpeg` and import a video into your test environment. This tool will allow you to test the GPU's video encoding and decoding processes to optimize the use of graphic effects and the overall graphics card.

```bash
sudo apt-get install ffmpeg
# Encoding a video
ffmpeg -y -vaapi_device /dev/dri/renderD128 -i <your_video.format> -vf 'format=nv12,hwupload' -c:v h264_vaapi -b:v 2M <your_output.format>
# Decoding a video
ffmpeg -y -hwaccel vaapi -vaapi_device /dev/dri/renderD128 -i <your_video.format> -vf 'format=nv12,hwupload' -c:v h264_vaapi <your_output.format>
```
