# AMD GPU plugin

Allows to measure AMD GPU hardware metrics with the ROCm software and AMD SMI library.

## Table of Contents

- [Description](#description)
- [Requirement](#requirement)
- [Configuration](#configuration)
- [Use](#use)

### Description

The new `plugin-amdgpu` currently allows to detect GPUs based on AMD architecture installed on a machine, and collect the following metrics about each of them :

> - **Average energy consumption** : Calculates and average between 2 measurement point base on the consumed energy since the last boot.
> - **Average power consumption** : Calculates the electrical power consumption.
> - **Memory used** : Retrieves Video computing memory (VRAM) and Graphic Table Translation (GTT) memories usage in MB.
> - **Thermal zone** : AMD GPUs provide various sensors to locating precisely the temperature by zone (VRAM, Hotspot, PCI bus, High Bandwidth Memory...)
> - **Clocks frequencies** : AMD GPUs provide various clocks about their computing units.
> - **PCI bus data consumption** : Retrieves sent and retrieved data.
> - **Engine usage** : Retrieves graphic units like GFX activity.
> - **Computing process** : Retrieves information about compute process (number, PID, VRAM usage...)

### Requirement

For the integration of AMD GPU plugin in ALUMET, we must using the Rust interface provided by the AMD SMI library (https://github.com/ROCm/amdsmi/tree/amd-mainline/rust-interface). However, we don't currently have an installable and usable Rust crate on https://crates.io/ for compilation, like any library in the project. So to compile this plugin like any other, we need to place ourselves in the `agent` directory from the ALUMET github repository, and also integrate and install first the AMD SMI library on the machine that compiling ALUMET, to do that you can follow the command lines bellow :

```bash
apt-get update && apt-get install -y apt-utils libdrm-dev cmake
cd ~/
git clone https://github.com/ROCm/amdsmi.git && cd amdsmi/ && mkdir build/
cmake .. && make -j$(nproc) && make install
```

### Configuration

In the `alumet-config.toml` used by ALUMET main program as configuration file, you can modify the parameters of this section to set the activation or deactivation of the plugin, and the time interval to collect metrics.

```rust
[plugins.amdgpu]
poll_interval = "2s"
enable = true
```

### Use

After the installation succeed, we can compiling ALUMET to generate the usable `alumet-agent` binary file.

```bash
cargo build --release -p "alumet-agent" --bins --all-features
```

The binary was finally created and is located in your ALUMET repository in `../alumet/target/release/alumet-agent` folder. To start correctly ALUMET program on the machine intended to collect AMD GPU metrics, we need to install amd-smi on system, and just run the binary `alumet-agent`.

```bash
apt-get install amd-smi-lib
yum install amd-smi-lib
```

Optionally, if it doesn't directly working in reason to an openshared library error on system, you can try this configuration :

```bash
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:/opt/rocm/lib:/opt/rocm/lib64
```

You can see now the result of collected metrics stored by default in the `alumet-output.csv` file.
