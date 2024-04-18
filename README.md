# ALUMET

**Adaptive, Lightweight, Unified Metrics.**

ALUMET is a generic measurement tool that brings together research and industry. Learn more on [the website](https://alumet.dev).

## This repository

ALUMET is divided in several parts:
- The `alumet` crate contains the core of the measurement tool, as a Rust library.
- Binaries can be created from this library, in order to provide a runnable measurement software, such as `app-agent`.
- Plugins are defined in separate folders: `plugin-nvidia`, `plugin-rapl`, etc.
- Two more crates, `alumet-api-dynamic` and `alumet-api-macros`, ease the creation of dynamic plugins written in Rust.

## License

Copyright 2024 Guillaume Raffin, BULL SAS, CNRS, INRIA, Grenoble INP-UGA.
Licensed under the EUPL-1.2 or later.
