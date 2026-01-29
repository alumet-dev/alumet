# AMD GPU plugin

Allows to measure AMD GPU hardware metrics with the ROCm software and `amdsmi` library.
The new `plugin-amdgpu` currently allows you to detect AMD architecture-based GPUs installed on a machine, and collect the following metrics on each of them :

## Requirements

- Linux operating system.
- AMD GPU(s).
- Installation of libdrm.
- Installation of clang and libclang.
- Installation of `amdsmi` software : https://rocm.docs.amd.com/projects/install-on-linux/en/latest/install/quick-start.html and if it doesn't work properly on your linux distribution:
  - Installation of a recent [cmake](https://cmake.org/) version.
  - Build `amdsmi` [source](https://github.com/ROCm/amdsmi.git) code manually with recent cmake version (otherwise the builder will fail to compile and show the version you need to have).
- Set and configure some permissions on system to run properly all metrics: https://rocm.docs.amd.com/projects/install-on-linux/en/latest/install/prerequisites.html#configuring-permissions-for-gpu-access.

## Metrics

Here are the metrics collected by the plugin source:

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`amd_gpu_activity_usage`|Gauge|percentage|GPU activity usage|GPU|LocalMachine|[activity_type](#activity_type)|
|`amd_gpu_energy_consumption`|CounterDiff|millijoule|Average between 2 measurement points based on the energy consumed since the last start-up|GPU|LocalMachine||
|`amd_gpu_memory_usage`|Gauge|megabyte|Video compute memory (VRAM) and graphics table translation memory (GTT) usage|GPU|LocalMachine|[memory_type](#memory_type)|
|`amd_gpu_power_consumption`|Gauge|watt|Estimated average electricity consumption|GPU|LocalMachine||
|`amd_gpu_temperature`|Gauge|celsius|Values ​​from AMD GPUs equipped with different sensors to precisely locate temperature by zone|GPU|LocalMachine|[thermal_zone](#thermal_zone)|
|`amd_gpu_voltage`|Gauge|millivolt|Electric power consumption by a AMD GPU|GPU|LocalMachine||
|`amd_gpu_process_memory_usage`|Gauge|byte|Process memory usage|process|pid|[process_name](#process_name)|
|`amd_gpu_process_engine_usage_encode`|Gauge|nanosecond|Process GFX engine usage|process|pid|[process_name](#process_name)|
|`amd_gpu_process_engine_gfx`|Gauge|nanosecond|Process encode engine usage|process|pid|[process_name](#process_name)|
|`amd_gpu_process_memory_usage_cpu`|Gauge|byte|Process CPU memory usage|process|pid|[process_name](#process_name)|
|`amd_gpu_process_memory_usage_gtt`|Gauge|byte|Process GTT memory usage|process|pid|[process_name](#process_name)|
|`amd_gpu_process_memory_usage_vram`|Gauge|byte|Process VRAM memory usage|process|pid|[process_name](#process_name)|

### Attributes

#### activity_type

The activity type defines the type of unit or component use by an AMD GPU :

|Value|Description|
|-----|-----------|
|`graphic_core`|Main graphic core of AMD GPU|
|`memory_management`|Manage memory access and addresses translation|
|`unified_memory_controller`|Memory controller managing access to VRAM in organising writing/reading operations|

#### memory_type

The memory type defines the type of consumed memory by an AMD GPU :

|Value|Description|
|-----|-----------|
|`memory_graphic_translation_table`|Buffer memory for system management used as interface between GPU and system memory|
|`memory_video_computing`|GPU dedicated and integrated memory video to store graphics data for rendering|

#### thermal_zone

The architecture of AMD GPUs is broken down into several type zones associated with a thermal sensor, to analyse precisely the GPU hardware temperature :

|Value|Description|
|-----|-----------|
|`thermal_global`|The global temperature measured on a AMD GPU hardware|
|`thermal_hotspot`|Value measured by a probe able to locating the maximal temperature on a AMD GPU hardware|
|`thermal_high_bandwidth_memory_X`|Temperature measured on GPU equipped with High Bandwidth Memory, designed to deliver high data transfer while minimizing power consumption in same time. Each "X" index (0 to 3) corresponding to a specific HBM stack|
|`thermal_pci_bus`|Temperature concerning only the data BUS PCI corresponding to the interface between GPU and others components|

#### process_name

|Value|Description|
|-----|-----------|
|`process_name`|ASCII table which defines the name in process parameters, converted in common UTF-8 encoded string|

## Configuration

Here is a configuration example of the plugin. It's part of the ALUMET configuration file (eg: `alumet-config.toml`).

```toml
[plugins.amd-gpu]
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

- First, you can install `hipcc` compiler, and use it to compile a small C++ program usign a HIP kernel on a GPU, to perform various tasks on it :

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

- Secondly, you can install `ffmpeg` and import a testing video into your test environment. This tool will allow you to test the GPU's video encoding and decoding processes to optimize the use of graphic effects and the overall graphics card. Run it at the same time as the alumet agent is running:

```bash
ffmpeg -y -vaapi_device /dev/dri/renderD128 -i <your_video.format> -vf 'format=nv12,hwupload' -c:v h264_vaapi -b:v 2M <your_output.format>
ffmpeg -y -hwaccel vaapi -vaapi_device /dev/dri/renderD128 -i <your_video.format> -vf 'format=nv12,hwupload' -c:v h264_vaapi <your_output.format>
```
