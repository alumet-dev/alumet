# Energy to Carbon

A transform plugin which convert energy metrics (Joules) to carbon equivalent emission (gCO₂), using the following formula:

```math
\Large {Emission} = {Energy} \times {Emission\_Intensity}
```

- $`{Emission}`$ **(gCO₂)**: Carbon footprint of the machine.
- $`{Energy}`$ **(kWh)**: Energy consumed by the machine.
- $`{Emission\_intensity}`$ **(gCO₂/kWh)**: CO₂ emission factor of the energy source. 

## Requirements

In order to work, this plugin needs an Alumet measurement plugin which exports metrics in Joules like `rapl` or `energy-estimation-tdp`.

## Metrics

Here are the metrics created by the transform plugin.

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|More information|
|----|----|----|-----------|--------|----------------|----------|----------------|
|`carbon_emission`|Gauge|gCO₂|CO₂ emission estimations|LocalMachine|LocalMachine|||

## Configuration
The configuration file differs depending on which mode you want to use. Here is an example of how to configure this plugin for each of them. Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

### Country Mode
If you want to use the emission intensity of a specific country, use this mode:
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

### Override Mode
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

### World Average Mode
If you have no knowledge about your energy mix, you can just use the world average emission intensity.
``` toml
[plugins.energy-to-carbon]
# Time between each activation of the energy source (e.g. "1s", "500ms", "2m")
poll_interval = "2s"
# "country", "override" or "world_avg"
mode = "world_avg"  # Will set emission_intensity to 475 gCO₂/kWh
```

## More information

This plugin create a new metric for each metrics in Joules it received (it supports prefixed units). The new metric's name is always `carbon_emission`.

The country mode uses the `./plugins/energy-to-carbon/src/intensity/country/energy_mix_per_country.json` file to retrieve the emission intensity based on the country code you provide. Please note that these values are yearly calculated averages and may not be up to date.