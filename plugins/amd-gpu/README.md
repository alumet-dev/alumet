# AMD GPU plugin

Allows to measure AMD GPU hardware metrics with the ROCm software and `amdsmi` library.
The new `plugin-amdgpu` currently allows you to detect AMD architecture-based GPUs installed on a machine, and collect the following metrics on each of them :

## Requirements

- Linux operating system
- Installation of the graphic library [libdrm](https://github.com/Distrotech/libdrm).
- Build manually the `amdsmi` with [cmake](https://cmake.org/) DEB package based distro. Generally, RPM package based distro have already in their repository the `amdsmi` installable as classic package. It is important to have a recent version of cmake, otherwise the builder will fail to compile and describe with log messages the version you need.

```bash
sudo apt-get update && sudo apt-get install -y apt-utils libdrm-dev cmake
git clone https://github.com/ROCm/amdsmi.git
mkdir amdsmi/build/ && cd amdsmi/build/
cmake .. && make -j$(nproc) && make install
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:/opt/rocm/lib
```

- On any machine, you must have root permissions to run properly all metrics, and need to be included in groups `render` and `video` (especially for processes information, power and energy consumptions).

```bash
sudo usermod -aG render,video $USER
exec su -l $USER
```

## Metrics

Here are the metrics collected by the plugin source:

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`amd_gpu_energy_consumption`|CounterDiff|millijoule|Average between 2 measurement points based on the energy consumed since the last start-up|GPU|LocalMachine||
|`amd_gpu_engine_usage`|Gauge|percentage|Graphics units such as GFX activity (especially concerning graphic tasks)|GPU|LocalMachine||
|`amd_gpu_memory_usage`|Gauge|megabyte|Video compute memory (VRAM) and graphics table translation memory (GTT) usage|GPU|LocalMachine|[memory_type](#memory_type)|
|`amd_gpu_temperature`|Gauge|celsius|Values ​​from AMD GPUs equipped with different sensors to precisely locate temperature by zone|GPU|LocalMachine|[thermal_zone](#thermal_zone)|
|`amd_gpu_power_consumption`|Gauge|watt|Estimated average electricity consumption|GPU|LocalMachine||
|`amd_gpu_process_memory_usage`|Gauge|byte|Process memory usage|process|pid||
|`amd_gpu_process_engine_usage_encode`|Gauge|nanosecond|Process GFX engine usage|process|pid||
|`amd_gpu_process_engine_gfx`|Gauge|nanosecond|Process encode engine usage|process|pid||
|`amd_gpu_process_memory_usage_cpu`|Gauge|byte|Process CPU memory usage|process|pid||
|`amd_gpu_process_memory_usage_gtt`|Gauge|byte|Process GTT memory usage|process|pid||
|`amd_gpu_process_memory_usage_vram`|Gauge|byte|Process VRAM memory usage|process|pid||

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
# Time between each activation of the counter source.
poll_interval = "1s"
# On startup, the plugin inspects the GPU devices and detect their features.
# If `skip_failed_devices = true`, inspection failures will be logged and the plugin will continue.
# If `skip_failed_devices = false`, the first failure will make the plugin's startup fail.
skip_failed_devices = true
```

## More information

**IMPORTANT** : Although this plugin works by returning metrics, it remains experimental and the functions and values returned by the `amdsmi` library change regularly. There are some bugs in the values and some functions are not working, and are being fixed by the `amdsmi` and ROCm development team.

If you want to thoroughly test the AMD GPU plugin, especially to observe the usage of the GPU and its specific engines and units by process, in increasing the number of running process on an AMD GPU:

- You can first edit and run a small C++ program with the `hipcc` compiler, to run a HIP kernel on a GPU, to perform various tasks on it :

```cpp
#include <stdio.h>
#include "hip/hip_runtime.h"

#define DIM 1024

__global__ void multiply(double *A, int n) {
    int id = blockDim.x * blockIdx.x + threadIdx.x;
    if (id < n) {
        A[id] = 2.0 * A[id];
    }
}

int main() {
    int N = DIM * DIM;
    size_t size = N * sizeof(double);

    double *h_A = (double*)malloc(size);
    for (int i = 0; i < N; i++) {
        h_A[i] = (double)i;
    }

    double *d_A;
    hipMalloc(&d_A, size);
    hipMemcpy(d_A, h_A, size, hipMemcpyHostToDevice);

    int threadsPerBlock = 256;
    int blocksPerGrid = (N + threadsPerBlock - 1) / threadsPerBlock;
    multiply<<<blocksPerGrid, threadsPerBlock>>>(d_A, N);
    hipMemcpy(h_A, d_A, size, hipMemcpyDeviceToHost);

    free(h_A);
    hipFree(d_A);

    return 0;
}
```

You need to install the `hipcc` package and library to run a HIP kernel on a GPU with this program, at the same time as the alumet agent is running:

```bash
sudo apt-get install hipcc
hipcc -o <your_binary> <your_program.cpp>
./<your_binary>
```

- Secondly, You can install `ffmpeg` and import a testing video into your test environment. This tool will allow you to test the GPU's video encoding and decoding processes to optimize the use of graphic effects and the overall graphics card. Run it at the same time as the alumet agent is running:

```bash
sudo apt-get install ffmpeg
# Encoding a video
ffmpeg -y -vaapi_device /dev/dri/renderD128 -i <your_video.format> -vf 'format=nv12,hwupload' -c:v h264_vaapi -b:v 2M <your_output.format>
# Decoding a video
ffmpeg -y -hwaccel vaapi -vaapi_device /dev/dri/renderD128 -i <your_video.format> -vf 'format=nv12,hwupload' -c:v h264_vaapi <your_output.format>
```
