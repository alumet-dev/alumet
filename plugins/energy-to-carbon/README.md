# Energy to Carbon

A transform plugin that produces carbon equivalent emission (gCO₂) from energy measurements (Joule) using the following formula:

```math
\Large {Emission} = \frac{Energy}{3\,600\,000} \times {Emission\_Intensity}
```

- $`{Emission}`$ **(gCO₂)**: Carbon footprint of the machine.
- $`{Energy}`$ **(J)**: Energy consumed by the machine. Divided by 3,600,000 to convert from Joules to kWh.
- $`{Emission\_intensity}`$ **(gCO₂/kWh)**: CO₂ emission factor of the energy source.

## Requirements

In order to work, this plugin needs an Alumet measurement plugin that collects metrics in Joules like `rapl` or `energy-estimation-tdp`.

## Metrics

Here are the metrics created by the transform plugin.

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|More information|
|----|----|----|-----------|--------|----------------|----------|----------------|
|`carbon_emission`|Gauge|gCO₂|CO₂ emission estimations|same as the input measurements|same as the input measurements|same as the input measurements||

## Configuration

The emission intensity can be computed using several methods. This plugin supports 3 modes, each relying on a different `IntensityProvider` to handle the computation.

Configuration varies by mode. Add one of the following sections to your Alumet agent configuration file (typically `alumet-config.toml`):

### Country Mode

If you want to use the emission intensity of a specific country, use this mode:

```toml
[plugins.energy-to-carbon]
# "country", "intensity_override" or "world_avg"
mode = "country"

[plugin.energy-to-carbon.country]
# Country 3-letter ISO Code
country = "FRA"
```

The country mode uses the `./plugins/energy-to-carbon/src/intensity/country/energy_mix_per_country.json` file to retrieve the emission intensity based on the country code you provide. Please note that these values are yearly calculated averages and may not be up to date.

### Override Mode

If you know your exact emission intensity, you can override it.

```toml
[plugins.energy-to-carbon]
# "country", "intensity_override" or "world_avg"
mode = "intensity_override"

[plugin.energy-to-carbon.override]
# Override the emission intensity value (in gCO₂/kWh).
intensity = 100
```

### World Average Mode

If you have no knowledge about your energy mix, you can just use the world average emission intensity.

```toml
[plugins.energy-to-carbon]
# "country", "intensity_override" or "world_avg"
mode = "world_avg"  # Will set emission_intensity to 475 gCO₂/kWh
```
