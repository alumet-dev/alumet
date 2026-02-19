# Energy to Carbon plugin <!-- omit in toc -->

## Table of Contents <!-- omit in toc -->

- [Introduction](#introduction)
- [Energy to Carbon plugin](#energy-estimation-tdp-plugin)
  - [How to use](#how-to-use)
  - [Prepare your environment](#prepare-your-environment)
  - [Configuration](#configuration)

## Introduction

This plugin estimate ...

```math
\Large Energy=\frac{cgroupv2*cpu\_total\_usage*nb_{\text{vcpu}}*TDP}{10^6*pooling\_interval*nb_{\text{cpu}}}
```

- $`{cgroupv2*cpu\_total\_usage}`$: Total usage of CPU in micro seconds for a pod.
- $`nb_{\text{vcpu}}`$: Number of virtual CPU of the hosting machine where pod is running.
- $`nb_{\text{cpu}}`$: Number of physical CPU of the hosting machine where pod is running
- $`{polling\_interval}`$: Polling interval of cgroupv2 input plugin.

## Energy estimation tdp plugin

### How to use

Just compile the app-agent of the alumet's github repository.

```bash
cargo run
```

The binary created by the compilation will be found under the target repository.

### Prepare your environment

To work this plugin needs k8s plugin configured, so the needed things are related to k8s plugin requirements:

1. cgroup v2
2. kubectl
3. alumet-reader user

### Configuration

```toml
[plugins.energy-estimation-tdp]
...
```

- `pool_interval`: ...

To get the CPU capacity of a kubernetes node, execute the following command:

```bash

```