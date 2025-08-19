<h1 align="center">
    <img src="readme/alumet-banner.png" height="141px"></img>
</h1>

<a href="https://crates.io/crates/alumet"><img src="https://img.shields.io/crates/v/alumet?link=https%3A%2F%2Fcrates.io%2Fcrates%2Falumet"/></a>

**Adaptive, Lightweight, Unified Metrics.**

**Alumet** is a modular framework for local and distributed measurement.

## Overview

<div align="center">
    <img src="https://alumet-dev.github.io/user-book/images/alumet-high-level-view.svg"/>
</div>

Alumet provides a unified interface for gathering measurements with sources (on the left), transforming the data with models (in the middle) and writing the result to various outputs (on the right).
The elements (colored rectangles) are created by plugins, on top of a standard framework.

- Extensible Framework: Alumet can easily be extended in order to make new research experiments. Leverage existing plugins and only add what you need, without reinventing the wheel. Take advantage of the unified data model and parallel measurement pipeline.
- Operational Tool: the end result is (or aims to be) a ready-to-use measurement tool that is robust, efficient and scalable.

[Learn more on the website](https://alumet.dev).

## Use cases

Tools built with Alumet can be used for (non-exhaustive list):
- measuring the energy consumption of some hardware component (CPU, GPU, etc.) in an accurate and efficient way by using the latest research results[^1]
- local system monitoring (bare-metal HPC node, cloud VM, etc.)
- distributed monitoring (datacenters, K8S clusters, etc.)
- profiling a specific application

[^1]: See the following research paper for a detailed analysis of some common errors in RAPL-based measurement tools: [Guillaume Raffin, Denis Trystram. Dissecting the software-based measurement of CPU energy consumption: a comparative analysis. 2024. ⟨hal-04420527v2⟩](https://hal.science/hal-04420527).

## How to use

We provide a standard Alumet agent that you can install on your system(s). For the moment, Linux is the only supported OS. [Download the agent](https://github.com/alumet-dev/alumet/releases/latest) from the latest release.

Please read the [Alumet User Book](https://alumet-dev.github.io/user-book/) to learn how to install and use the Alumet "agent" (the program that performs the measurements).

If you have a question, feel free to ask on the [Discussions page](https://github.com/alumet-dev/alumet/discussions).

## Extending Alumet

The [alumet](https://crates.io/crates/alumet) crate, aka "Alumet core", provides the measurement framework in the form of a library.
This library offers a plugin system that you can use to extend Alumet in the following ways:
- read new sources of measurements
- apply arbitrary transformations to the data (such as energy attribution models)
- export the data to new outputs
- perform actions on startup and shutdown

Please read the [Alumet Developer Book](https://alumet-dev.github.io/developer-book/) to learn how to make plugins and agents.

## Contributing

Alumet is a joint project between the LIG (computer science laboratory of Grenoble) and Eviden (Atos HPC R&D). It is also open to external volunteers like you!

Please go to the [contributing guide](./CONTRIBUTING.md) to get started (work in progress).

## License

Copyright 2024 Guillaume Raffin, BULL SAS, CNRS, INRIA, Grenoble INP-UGA.
Licensed under the EUPL-1.2 or later.

You can find [more information about the EUPL here](https://joinup.ec.europa.eu/collection/eupl/introduction-eupl-licence). The EUPL is compatible with many other open source licenses and shares some principles with the well-known LGPL.
