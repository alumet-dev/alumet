# Energy to Carbon <!-- omit in toc -->

## Table of Contents <!-- omit in toc -->

- [Introduction](#introduction)
- [Energy to Carbon plugin](#energy-estimation-tdp-plugin)
  - [How to use](#how-to-use)
  - [Prepare your environment](#prepare-your-environment)
  - [Configuration](#configuration)

## Introduction

This plugin estimate...

```math
\Large {Emission} = {Energy} \times {Emission\_Intensity}
```

- $`{Emission}`$ **(gCO₂)**: Carbon footprint of the pod.
- $`{Energy}`$ **(kWh)**: Energy consumed by the pod.
- $`{Emission\_intensity}`$ **(gCO₂/kWh)**: CO₂ emission factor of the energy source.


## Energy estimation tdp plugin

### Prepare your environment

To work this plugin needs k8s plugin configured, so the needed things are related to k8s plugin requirements:

### How to use

Just compile the app-agent of the alumet's github repository.

```bash
cargo run
```

The binary created by the compilation will be found under the target repository.


### How is $`{Emission\_intensity}`$ estimated ?

We use a cascading method, trying to use the most accurate method first and falling back to less precise ones if needed.

``` Bash
emission_intensity Cascade
|
├── 1. User-defined fixed value                  (custom, from own measurements)
│
├── 2. Country average from Country Code
│
└── 3. World average fallback                    (default, least accurate)
```

### Configuration

``` toml
[plugins.energy-to-carbon]
# Override the emission intensity value (in gCO₂/kWh).
emission_intensity_override = 100.0
# Country 3-letter ISO Code
country = "FRA"
# Time between each activation of the energy source (e.g. "1s", "500ms", "2m")
poll_interval = "2s"
# If true, replace joules metrics with carbon emission metrics. If false, keep both.
replace_metrics = false
```