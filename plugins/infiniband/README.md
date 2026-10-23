# InfiniBand plugin

Polls `/sys/class/infiniband` and reports, per InfiniBand port, the receive and transmit packet and byte rates.
Each port is exposed as a custom resource identified by its device name and port number, with one measurement per direction (`rx` and `tx`).

## Requirements

- Linux operating system.
- `/sys/class/infiniband` mounted and readable (it is part of the standard `sysfs` mount).
- A host driver that exposes the InfiniBand counters in sysfs. On Mellanox/NVIDIA adapters, this is provided by the MLX5 driver from the DOCA host stack.
  - On Kraken, the validated setup is DOCA 3.3.x with the IB devices `ibp97s0f1` and `ibp113s0f1` (port 1).

The InfiniBand ports are detected when the plugin starts, and the port list is refreshed on every poll: a port that appears after startup is picked up at the next poll, and a port that disappears stops being reported.

A host without InfiniBand is a configuration error: if `/sys/class/infiniband` is missing or unreadable, or if no port matches the configuration, the plugin fails the agent startup with an explicit error instead of collecting nothing forever.

Note that udev may rename the *network* interfaces associated to the adapter (for instance `ibs2f1` or `ibs1f0`), while the *InfiniBand* device names keep the `ibp*s*f*` form.
This plugin reads `/sys/class/infiniband`, so it relies on the InfiniBand device names and never on the network interface names.

## Metrics

Here are the metrics collected by the plugin source.

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|
|----|----|----|-----------|--------|----------------|----------|
|`infiniband_packets_per_second`|Gauge|Hz|Packets received or transmitted per second on the port|`Resource::Custom { kind: "infiniband_port", id: "<device>:<port>" }`|`LocalMachine`|[interface](#attributes), [port](#attributes), [direction](#attributes)|
|`infiniband_bytes_per_second`|Gauge|B (bytes; rate is per second, see name)|Bytes received or transmitted per second on the port|`Resource::Custom { kind: "infiniband_port", id: "<device>:<port>" }`|`LocalMachine`|[interface](#attributes), [port](#attributes), [direction](#attributes)|

### Attributes

|Attribute|Description|Example|
|---------|-----------|-------|
|`interface`|Name of the InfiniBand device, as found in `/sys/class/infiniband`|`ibp97s0f1`|
|`port`|Port number of the InfiniBand device, as an integer|`1`|
|`direction`|Direction of the traffic|`rx` or `tx`|

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.infiniband]
# Interval between two measurements.
poll_interval = "1s"
# Optional: root of the sysfs InfiniBand tree (defaults to /sys/class/infiniband).
# root_path = "/sys/class/infiniband"
# Optional: only monitor these InfiniBand devices (defaults to all devices found).
# devices = ["ibp97s0f1", "ibp113s0f1"]
# Optional: only monitor these port numbers (defaults to all ports).
# ports = [1]
```

## Sysfs counters

The plugin reads four cumulative counters per port, in `/sys/class/infiniband/<device>/ports/<port>/counters/`:

|Counter|Meaning|Unit|
|-------|-------|----|
|`port_rcv_packets`|Total number of packets received on all VLs (may include packets with errors)|packets|
|`port_xmit_packets`|Total number of packets transmitted on all VLs (may include packets with errors)|packets|
|`port_rcv_data`|Total number of data octets received on all VLs|4-octet units|
|`port_xmit_data`|Total number of data octets transmitted on all VLs|4-octet units|

The two data counters do not count octets: the InfiniBand architecture specification defines them as "the total number of data octets, divided by 4", and the Linux sysfs ABI documents the same wording.
The divisor is part of the counter definition, so it is always 4 for both flavours of the counter. It is not the link width, despite the "(lanes)" wording kept by the kernel ABI documentation, and it must not be derived from the negotiated width or speed.
The plugin multiplies the delta by 4 to report bytes.

### Why some counters come in two flavours (`*` and `*_64`)

The counters above are 32-bit wide and wrap around at 2^32 units, which a fast link can reach between two polls.
The InfiniBand specification therefore also defines an *extended* set of the same counters, 64 bits wide.
Stacks based on the NVIDIA/Mellanox OFED driver (and DOCA) expose them next to the legacy ones with a `_64` suffix, for example `port_rcv_data_64` beside `port_rcv_data`; both flavours keep the same unit, so the same conversion applies.
The plugin prefers the `_64` variant when the driver exposes it, and falls back to the legacy one otherwise, because a wrap-around on the legacy counter is indistinguishable from a counter reset and costs one skipped sample.

Sources:

- Linux kernel ABI: [`Documentation/ABI/stable/sysfs-class-infiniband`](https://www.kernel.org/doc/Documentation/ABI/stable/sysfs-class-infiniband) (section `counters/port_rcv_data`)
- Linux kernel implementation: [`drivers/infiniband/core/sysfs.c`](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/drivers/infiniband/core/sysfs.c) (`show_pma_counter`, extended counters)
- InfiniBand Architecture Specification, vol. 1: `PortCounters` and `PortCountersExtended`
- [`perfquery(8)`](https://linux.die.net/man/8/perfquery): "In PortCounters, PortCountersExtended, PortXmitDataSL and PortRcvDataSL, components that represent Data (...) indicate octets divided by 4 rather than just octets"
- NVIDIA UFM documentation, [InfiniBand port counters](https://networking-docs.nvidia.com/ufmsdnappswum/41815/infiniband-port-counters) ("Rcv Data (in bytes): total number of data octets, divided by 4 ... reported as a multiple of four octets")

## Limitations

- The first poll only records a baseline: rates become available from the second poll onwards.
- If a hardware counter is reset (or wraps around before the extended counter is available) between two polls, the corresponding sample is skipped instead of reporting an inconsistent rate. The next poll establishes a new baseline and resumes emission.
- Rates are averaged over the poll interval: a shorter `poll_interval` gives a finer view but reads sysfs more often.
- Unlike the `procfs` plugin's `network_bytes` metric, which reports the number of bytes transferred during the interval, this plugin reports per-second rates (`*_per_second`).
- Units are simple UCUM units (`Hz`, `B`): the relay protocol serializes a unit as a (base unit, prefix) pair and re-parses the base unit name with `Unit::from_str`, which only accepts built-in units, so a custom unit such as `By/s` cannot cross a relay. The per-second rate is carried by the metric name (`*_per_second`).

## OAR jobs

This plugin does **not** produce the `involved_jobs` attribute: see [OAR.md](OAR.md) to attach it with the [`oar`](../cgroups/oar/README.md) plugin.

## Architecture note

Port detection is a standalone function (`sysfs::discover_ports`, wrapped by `detect_ports` for the startup check), separate from the polling source that consumes it.
The core of Alumet is expected to grow a first-class detection API; until then, keeping detection independent of the source is what makes the migration a local change.

## Development note

This plugin was vibe-coded with the following multi-agent chain:

`scout: GRICAD/mellum2-12b-thinking · planner: GRICAD/ARES/DeepSeek-V4.1-Flash:high · worker: GRICAD/ARES/DeepSeek-V4.1-Flash:low · reviewer: GRICAD/mimo-v26-flash-mopd`
