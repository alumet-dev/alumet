# AMD GPU plugin

Allows to measure AMD GPU hardware metrics with the ROCm software and `rocm-smi` library.

## Requirements

- Linux operating system.
- Installation of `rocm-smi` package. On most Linux distributions, the package manager allows you to install `rocm-smi`, but this does not always work, so it is better to compile from source using the following steps:

```bash
sudo apt-get update && sudo apt-get install -y apt-utils cmake clang libdrm-dev libclang-dev
git clone https://github.com/ROCm/rocm-systems.git
mkdir rocm-systems/projects/rocm-smi-lib/build/ && cd rocm-systems/projects/rocm-smi-lib/build/
cmake .. && make -j$(nproc) && make install
export LD_LIBRARY_PATH=/usr/local/lib
```

- You must have each permissions on your system to run properly all metrics, to configure the correct environnement to test AMD GPU plugin, follow the steps described here: https://rocm.docs.amd.com/projects/install-on-linux/en/latest/install/prerequisites.html#configuring-permissions-for-gpu-access.

## Metrics

Here are the metrics collected by the plugin source:

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`amd_gpu_activity_usage`|Gauge|percentage|GPU hardware utilization|GPU|LocalMachine|[activity_type](#activity_type)|
|`amd_gpu_energy_consumption`|CounterDiff|millijoule|Average between 2 measurement points based on the energy consumed since the last start-up|GPU|LocalMachine||
|`amd_gpu_memory_usage`|Gauge|megabyte|Video compute memory (VRAM) and graphics table translation memory (GTT) usage|GPU|LocalMachine|[memory_type](#memory_type)|
|`amd_gpu_temperature`|Gauge|celsius|Values ​​from AMD GPUs equipped with different sensors to precisely locate temperature by zone|GPU|LocalMachine|[thermal_zone](#thermal_zone)|
|`amd_gpu_power_consumption`|Gauge|watt|Estimated average electricity consumption|GPU|LocalMachine||
|`amd_gpu_process_compute_unit_usage`|Gauge|percent|Process comput unit usage|process|pid||
|`amd_gpu_process_memory_usage_vram`|Gauge|byte|Process VRAM memory usage|process|pid||
|`amd_gpu_process_sdma_usage`|Gauge|microsecond|Process SDMA usage|process|pid||

### Attributes

#### activity_type

The activity type defines the unit or the nature of the GPU hardware used :

|Value|Description|
|-----|-----------|
|`graphic_core`|GFX utilization corresponding of the main graphic unit of an AMD GPU that release graphic tasks and rendering|
|`memory_management`|Unit responsible for managing and accessing VRAM, and coordinating data exchanges between it and the GPU|
|`unified_memory_controller`|Single memory address space accessible from any processor within a system|

#### memory_type

The memory type defines the type of consumed memory by GPU :

|Value|Description|
|-----|-----------|
|`memory_graphic_translation_table`|The GTT memory is a portion of system memory that can be used as a virtual extension when dedicated VRAM is insufficient, or in addition to it, and allowing to increase available graphics memory but with higher latency|
|`memory_video_computing`|The memory video computing is a fast memory integrated on the graphics card, storing graphics data for efficient rendering|

#### thermal_zone

The architecture of AMD GPUs is broken down into several type zones associated with a thermal sensor, to analyse precisely the GPU hardware temperature:

|Value|Description|
|-----|-----------|
|`thermal_global`|The global temperature measured on the AMD GPU hardware|
|`thermal_hotspot`|Value measured by a probe able to locating the maximal temperature on the AMD GPU hardware|
|`thermal_memory`|Temperature only emitted by video computing memory of the AMD GPU hardware|
|`thermal_high_bandwidth_memory_X`|Temperature measured on GPU equipped with High Bandwidth Memory, designed to deliver high data transfer while minimizing power consumption in same time. Each "X" index (0 to 3) corresponding to a specific HBM stack|

## Configuration

Here is a configuration example of the plugin. It's part of the ALUMET configuration file (eg: `alumet-config.toml`).

```toml
[plugins.amdgpu]
# Time between each activation of the counter source.
poll_interval = "1s"
# Initial interval between two flushing of AMD GPU measurements.
flush_interval = "5s"
# On startup, the plugin inspects the GPU devices and detect their features.
# If `skip_failed_devices = true`, inspection failures will be logged and the plugin will continue.
# If `skip_failed_devices = false`, the first failure will make the plugin's startup fail.
skip_failed_devices = true
```

## More information

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
