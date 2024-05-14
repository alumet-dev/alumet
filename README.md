# ALUMET

**Adaptive, Lightweight, Unified Metrics.**

ALUMET is a generic measurement tool that brings together research and industry.
Interesting features include:
- true modularity: you can add new sources of measurements, data transforms and outputs without modifying the core of the tool
- pay only for what you need: by choosing the plugins that are included in the tool, you avoid bloat and stay lightweight
- performance: built in Rust, optimized for low latency and low memory consumption
- unification: one core, one interface for all your metrics on all your devices

Learn more on [the website](https://alumet.dev).

## What can I measure with it?

Alumet sources include:
- RAPL counters on x86 CPUS
- NVIDIA dedicated GPUs
- NVIDIA Jetson devices
- Linux perf_events for processes and cgroups
- Kubernetes pods (WIP)

If your favorite feature is not listed above, don't worry! The list of plugins is rapidly growing, and we have an ambitious roadmap.

## How to use

Please read [the Alumet user book](https://alumet-dev.github.io/user-book/) to learn how to install and use Alumet.

## This repository

ALUMET is divided in several parts:
- The `alumet` crate contains the core of the measurement tool, as a Rust library.
- Binaries can be created from this library, in order to provide a runnable measurement software, such as `app-agent`.
- Plugins are defined in separate folders: `plugin-nvidia`, `plugin-rapl`, etc.
- Two more crates, `alumet-api-dynamic` and `alumet-api-macros`, ease the creation of dynamic plugins written in Rust (WIP).
- `test-dynamic-plugins` only exists for testing purposes.

## License

Copyright 2024 Guillaume Raffin, BULL SAS, CNRS, INRIA, Grenoble INP-UGA.
Licensed under the EUPL-1.2 or later.
