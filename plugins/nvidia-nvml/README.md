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
|`nvml_energy_consumption`|CounterDiff|milliJoule|Difference between 2 measurement points based on the consumed energy since the last boot|GPU|LocalMachine||
|`nvml_instant_power`|Gauge|milliWatt|Instant power consumption|GPU|LocalMachine||
|`nvml_temperature_gpu`|Gauge|Celsius|Main temperature emitted by a given device|GPU|LocalMachine||
|`nvml_gpu_utilization`|Gauge|Percentage (0-100)|GPU rate utilization|GPU|LocalMachine||
|`nvml_gpu_memory_info`|Gauge|bytes|GPU VRAM information. Only available with recent versions of NVIDIA drivers ($\geq510$)|GPU|LocalMachine|[kind](#kind)|
|`nvml_encoder_sampling_period`|Gauge|Microsecond|Current utilization and sampling size for the encoder|GPU|LocalMachine||
|`nvml_decoder_sampling_period`|Gauge|Microsecond|Current utilization and sampling size for the decoder|GPU|LocalMachine||
|`nvml_n_compute_processes`|Gauge|None|Relevant currently running computing processes data|GPU|LocalMachine||
|`nvml_n_graphic_processes`|Gauge|None|Relevant currently running graphical processes data|GPU|LocalMachine||
|`nvml_memory_utilization`|Gauge|Percentage|GPU memory utilization by a process|GPU|Process||
|`nvml_encoder_utilization`|Gauge|Percentage|GPU video encoder utilization by a process|GPU|Process||
|`nvml_decoder_utilization`|Gauge|Percentage|GPU video decoder utilization by a process|GPU|Process||
|`nvml_sm_utilization`|Gauge|Percentage|Utilization of the GPU streaming multiprocessors by a process (3D task and rendering, etc...)|GPU|Process||
|`nvml_clock_info`|Gauge|Hertz|GPU clock frequency|GPU|LocalMachine|[Clock_type](#clock_type)|
|`nvml_used_gpu_memory`|Gauge|bytes|Amount of used GPU memory|GPU or GPUPartition|Process|[Context](#context), [Compute_instance_ID](#compute_instance_id)|
|`nvml_gpm_graphics_util`|Gauge|Percentage|Percentage of time any warp was active on a multiprocessor|GPU|LocalMachine||
|`nvml_gpm_sm_util`|Gauge|Percentage|Percentage of time each multiprocessor had at least 1 warp assigned|GPU|LocalMachine||
|`nvml_gpm_sm_occupancy`|Gauge|Percentage|Percentage of warps that were active vs theoretical maximum|GPU|LocalMachine||
|`nvml_gpm_tensor_utilization`|Gauge|Percentage|Percentage of time the GPU's SMs were doing (any, DFMA, HMMA or IMMMA) tensor operations|GPU|LocalMachine|[Operation type](#operation_type)|
|`nvml_gpm_precision_utilization`|Gauge|Percentage|Percentage of time the GPU's SMs were doing integer, FP64, FP32 or FP16 operations|GPU|LocalMachine|[Precision](#precision)|
|`nvml_gpm_dram_utilization`|Gauge|Percentage|Percentage of DRAM bandwidth used|GPU|LocalMachine||
|`nvml_gpm_pcie_throughput`|Gauge|byte|PCIe bytes transmitted or received per second|GPU|LocalMachine|[Direction](#direction)|
|`nvml_gpm_nvdec_utilization`|Gauge|Percent|NVDEC utilization|GPU|LocalMachine|[Instance](#instance)|
|`nvml_gpm_nvjpg_utilization`|Gauge|Percent|NVJPG utilization|GPU|LocalMachine|[Instance](#instance)|
|`nvml_gpm_nvofa_utilization`|Gauge|Percent|NVOFA utilization|GPU|LocalMachine|[Instance](#instance)|
|`nvml_gpm_nvlink_throughput`|Gauge|bytes|NVLink received and transmitted bytes per second across all links.|GPU|LocalMachine|[Instance](#instance), [Direction](#direction)|

Some metrics can be disabled, see the `mode` configuration option.

### Attributes

#### Kind

The kind of the memory is the type of the allocated memory space reserved by the system or the hardware.

|Value|Description|
|-----|-----------|
|`free`|Unallocated device memory|
|`total`|Total physical device memory|
|`used`|Allocated device memory|
|`reserved`|Device memory (in bytes) reserved for system use (driver or firmware)|

#### Clock_type

The speed of the clock may vary depending on the type. These are the available types:

|Value|Description|
|-----|-----------|
|`SM`|Streaming Multiprocessor (compute units) clock domain|
|`Video`|Video encoder/decoder clock domain|
|`Graphics`|Graphics clock domain|
|`Memory`|Memory clock domain|

#### context

There are two possible contexts for processes:

|Value|Description|
|-----|-----------|
|`graphics`|Graphics based processes (OpenGL, DirectX, etc.).|
|`compute`|Compute processes (a CUDA application, etc.).|

#### compute_instance_id

Attribute containing the compute instance ID of the process. Only available when MIG (Multi Instance GPU) is enabled.

#### operation_type

The type of operation done by the tensor cores :

|Value|Description|
|-----|-----------|
|`dfma`|DFMA operations (Dynamic Fused Multiply-Accumulate)|
|`hmma`|HMMA operations (Half-precision Matrix Multiply-Accumulate)|
|`imma`|HMMA operations (Integer Matrix Multiply-Accumulate)|
|`any`|All of the above|

#### precision

The precision of the input of the operations done :

|Value|Description|
|-----|-----------|
|`integer`|Integer (of any size)|
|`fp64`|Floating point 64 bits|
|`fp32`|Floating point 32 bits|
|`fp16`|Floating point 16 bits|

#### direction

The direction of the bytes throughput :

|Value|Description|
|-----|-----------|
|`transmitted`|Bytes are sent through the link|
|`received`|Bytes are received through through the link|

#### instance

The identifier of the component, used when there are multiple ones :

|Value|Description|
|-----|-----------|
|`0 (...) 17`|The number of the component|
|`total`|The total value averaged on every components of that type|

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

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

# See below
mode = "full"

gpm_metrics = []
```

### Choosing the Right Mode

The NVML plugin offers two modes: `full` and `minimal`.

In `full` mode, all the metrics listed in the table above are provided (if they are available on the GPU).

If you want to make the GPU measurement faster, you can use the `minimal` mode.

In `minimal` mode, only `nvml_energy_consumption` and `nvml_instant_power` are provided.
The only measured value is `nvml_instant_power`. It is used to estimate `nvml_energy_consumption`.
The `minimal` mode only works on GPU that support the `nvmlDeviceGetPowerUsage` device query (the plugin detects if this is the case on startup).

### GPM metrics

GPM metrics are more precise metrics only available on recent GPUs (Hopper and forward). You have to enable `full` mode to enable them. For more details on what each metric mean, please refer to the [NVML API documentation](https://docs.nvidia.com/deploy/nvml-api/group__nvmlGpmEnums.html#group__nvmlGpmEnums). You can choose which GPM metrics to monitor by filling the array `gpm_metrics`. Here are the available metrics :

**Tensor operations related metrics:**

`AnyTensorUtil`, `DfmaTensorUtil`, `HmmaTensorUtil`, `ImmaTensorUtil`

**DRAM related metrics:**

`DramBwUtil`

**Generic computation related metrics:**

`GraphicsUtil`, `SmUtil`, `SmOccupancy`

Or by precision: `Fp64Util`, `Fp32Util`, `Fp16Util`, `IntegerUtil`

**PCIE throughput:**

`PcieTxPerSec`, `PcieRxPerSec`

**NVDEC utilization:**

`Nvdec0Util` [...] `Nvdec7Util`

**NVJPG utilization:**

`Nvjpg0Util` [...] `Nvjpg7Util`

**NVOFA utilization:**

`Nvofa0Util`, `Nvofa1Util`

**NVLink throughput:**

Received: `NvlinkTotalRxPerSec`, `NvlinkL0RxPerSec` [...] `NvlinkL17RxPerSec`

Transmitted:  `NvlinkTotalTxPerSec`, `NvlinkL0TxPerSec` [...] `NvlinkL17TxPerSec`

## More information

Not all software use the GPU to its full extent.
For instance, to obtain non-zero values for the video encoding/decoding metrics, use a video software like `ffmpeg`.

### GPU counter updates

NVML requires 20-100ms to refresh counter values based on GPU model.
When `poll_interval` is set too low, the plugin queries identical counter values repeatedly during polling.
Since some measurements are calculated from previous polls, these measurements are discarded rather than reported as zero values.

## Note on NVIDIA 'utilization'

When NVIDIA measures 'utilization', they actually mean the percent of time that the resource was solicited over the last sample of time.

For example, if *memory utilization* $= 60\%$, then it means that during the last **sample of time** (for instance, the past 1 second), 60% of **that time** ($=600ms$) was spent reading or writing the memory.

This is also true for `nvml_gpu_utilization`, `nvml_memory_utilization`, `nvml_encoder_utilization`, `nvml_decoder_utilization` and `nvml_sm_utilization`.

However, `nvml_used_gpu_memory` refers to the VRAM usage per process in bytes.
