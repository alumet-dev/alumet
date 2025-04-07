# Alumet core: modular measurement framework

Alumet allows you to build efficient and robust measurement tools by assembling plugins.

## Overview

The `alumet` crate (casually referred to as "Alumet core") provides:
- A **unified data model** for platform-agnostic measurement. You can define new metrics and tag your measurements with valuable information.
- A **parallel measurement pipeline** based on async Rust. The pipeline connects measurements sources, transform functions and outputs together. Elements are as isolated as possible, so that one failure cannot take the whole measurement tool down.
- A **plugin system** for extending Alumet in your own way and reusing existing open-source plugins. Gathering new data is as simple as creating a plugin, implementing the `Source` trait and adding your source to the pipeline.
- **Transformation helpers**, which ease the creation of estimation models, filtering steps, and more.
- **Testing utilities**, which facilitate the end-to-end testing of Alumet plugins.
- An **agent API** for building standalone binaries called "agents" in a few lines of code. Alumet agents are the operational measurement tools that you run wherever you need to, i.e. the "software monitoring tools".

Please refer to the crate's documentation for more information.

## Use cases

Tools built with Alumet can be used for (non-exhaustive list):
- measuring the energy consumption of some hardware component (CPU, GPU, etc.)
- local system monitoring (bare-metal HPC node, cloud VM, etc.)
- distributed monitoring (datacenters, K8S clusters, etc.)
- profiling a specific application

## Guidelines/Tutorial

On top of the crate's documentation, the [Alumet Developer Book](https://alumet-dev.github.io/developer-book/) provides useful guidelines and a step-by-step tutorial for creating plugins and agents.

## I just want to install Alumet and measure things, how do I do that?

The `alumet` crate is for building plugins and agents.
If you just want to install the standard, ready-to-use **Alumet agent**, which bundles various plugins, [download it from here](https://github.com/alumet-dev/alumet/releases/latest).
Please read the [Alumet User Book](https://alumet-dev.github.io/user-book/) to learn how to use it.
