# Energy attribution plugin

The energy-attribution plugin combines measurements related to the energy consumption of some hardware components with measurements related to the use of the hardware by the software.

It computes a value per resource per consumer, using the formula of your choice (configurable).

## Requirements

To obtain hardware and software measurements, you need to enable other plugins such as `rapl` or `tdp` for energy consumption (per resource) and `procfs` or any cgroup plugins (`K8s`, `OAR`, `Slurm`) for hardware usage (per consumer).

## Metrics

This plugin creates new measurements based on its configuration.

|Name|Type|Unit|Description|Resource|ResourceConsumer|Attributes|More information|
|----|----|----|-----------|--------|----------------|----------|----------------|
|chosen by the config| Gauge | Joules | attributed energy | depends on the config| depends on the config|same as the input measurements||

## Configuration

Some use cases for this plugin are:

- Monitoring host with access to RAPL:
  - Per resource: RAPL plugin
  - Per consumer: procfs or any cgroup plugins (K8s, OAR, Slurm)
- Monitoring a host without RAPL (bare metal, ARM):
  - Per resource: TDP plugin which at the same time requires procfs
  - Per consumer: Any cgroup plugins (K8s, OAR, Slurm)

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

In this example, we define an attribution formula that produces a new metric `attributed_energy` by combining `cpu_energy` and `cpu_usage`.

```toml
[plugins.energy-attribution.formulas.attributed_energy]
# the expression used to compute the final value
expr = "cpu_energy * cpu_usage / 100.0"
# the time reference: this is a timeseries, defined by a metric (and other criteria, see below), that will not change during the transformation. Other timeseries can be interpolated in order to have the same timestamps before applying the formula.
ref = "cpu_energy"
# The duration the measurement points are kept in memory before being dropped.
# This need to be coherent with the poll_interval of the metrics involved in this formula.
# Eg: If the metrics come from sources that poll every 1 second, it's recommanded to define the retention_time to at least 2 seconds.
# This way the plugin can make use of the precedent values of this metric to make interpolations.
retention_time = "1s"

# Timeseries related to the resources.
[plugins.energy-attribution.formulas.attributed_energy.per_resource]
# Defines the timeseries `cpu_energy` that is used in the formula, as the measurement points that have:
# - the metric `rapl_consumed_energy`,
# - and the resource kind `"local_machine"`
# - and the attribute `domain` equal to `package_total`
cpu_energy = { metric = "rapl_consumed_energy", resource_kind = "local_machine", domain = "package_total" }

# Timeseries related to the resource consumers.
[plugins.energy-attribution.formulas.attributed_energy.per_consumer]
# Defines the timeseries `cpu_usage` that is used in the formula, as the measurements points that have:
# - the metric `cpu_percent`
# - the attribute `kind` equal to `total`
cpu_usage = { metric = "cpu_percent", kind = "total" }
```

You can configure multiple formulas. Be sure to give each formula a unique name.
For instance, you can have a table `formulas.attributed_energy_cpu` and a table `formulas.attributed_energy_gpu`.

## More information

Here is how the interpolation used by this plugin works.
Given a reference timeseries and some other timeseries, it synchronizes all the timeseries by interpolating the non-reference points at the timestamps of the reference. The reference is left untouched.

![Multivariate interpolation diagram](diagrams/alumet%20multivariate%20timeseries%20interpolation%20-%20inputs%20and%20result.png)
