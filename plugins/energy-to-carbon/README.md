# Energy to Carbon <!-- omit in toc -->

## Table of Contents <!-- omit in toc -->

- [Introduction](#introduction)
- [Energy to Carbon plugin](#energy-to-carbon-plugin)
  - [Prepare your environment](#prepare-your-environment)
  - [How to use](#how-to-use)
  - [Configuration](#configuration)

## Introduction

This plugin estimate...

```math
\Large {Emission} = {Energy} \times {Emission\_Intensity}
```

- $`{Emission}`$ **(gCO₂)**: Carbon footprint of the machine.
- $`{Energy}`$ **(kWh)**: Energy consumed by the machine.
- $`{Emission\_intensity}`$ **(gCO₂/kWh)**: CO₂ emission factor of the energy source.


## Energy to Carbon plugin

### Prepare your environment

In order to work, this plugin needs an Alumet measurement plugin which exports metrics in Joules like `rapl` or `energy-estimation-tdp`.


### How to use

Just compile the app-agent of the alumet's github repository.

```bash
cargo run
```

The binary created by the compilation will be found under the target repository.


### How is $`{Emission\_intensity}`$ estimated ?

You have 3 modes of execution depending on the knowledge you have of your own energy mix. The emission intensity will be computed differently depending on the one you choose.

Here are the configs for each mode you need to add to your `alumet-config.toml`.


### Configuration

#### Country Mode
It uses the `./plugins/energy-to-carbon/src/intensity/country/energy_mix_per_country.json` file to retrieve the emission intensity based on the country code you provide.
> Note that these values are yearly calculated averages and may not be up to date.
``` toml
[plugins.energy-to-carbon]
# Time between each activation of the energy source (e.g. "1s", "500ms", "2m")
poll_interval = "2s"
# "country", "override" or "world_avg"
mode = "country"

[plugin.energy-to-carbon.country]
# Country 3-letter ISO Code
country = "FRA"
```

#### Override Mode
If you know your exact emission intensity, you can override it.
``` toml
[plugins.energy-to-carbon]
# Time between each activation of the energy source (e.g. "1s", "500ms", "2m")
poll_interval = "2s"
# "country", "override" or "world_avg"
mode = "override"

[plugin.energy-to-carbon.override]
# Override the emission intensity value (in gCO₂/kWh).
intensity = 100
```

#### World Average Mode
If you have no knowledge about your energy mix, you can just use the world average emission intensity.
``` toml
[plugins.energy-to-carbon]
# Time between each activation of the energy source (e.g. "1s", "500ms", "2m")
poll_interval = "2s"
# "country", "override" or "world_avg"
mode = "world_avg"  # Will set emission_intensity to 475 gCO₂/kWh
```