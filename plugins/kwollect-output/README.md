# Kwollect plugin

The `kwollect` plugin pushes data to a [Kwollect](https://gitlab.inria.fr/grid5000/kwollect) API.

In particular, it allows users of the [Grid'5000](https://www.grid5000.fr/w/Grid5000:Home) testbed to easily visualize the measurements obtained by Alumet, alongside other measurements provided by, for instance, wattmeters.

## Requirements

You need access to the Kwollect API.

If you are running a job on Grid'5000, you already have this access.

## Configuration

Here is a configuration example of the `kwollect` plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.kwollect]
# Url of the Grid'5000 API, it needs to specify the correct site (here 'grenoble')
url = "https://api.grid5000.fr/stable/sites/grenoble/metrics"
# Name of the machine
hostname = "mars"
# Login and password used to push the metric, both are optional. If none are specified, it will push using the current user
# login = ""
# password = ""
```

On Grid'5000, you can simply generate this configuration (see below).

## How to use in a Grid'5000 job

<!-- markdownlint-disable MD029 -->

Here is a quick guide to send Alumet measurements to Kwollect.

1. Start a job with `oarsub`.
2. Automatically generate the configuration corresponding to your node:

```sh
alumet-agent --plugins rapl,kwollect   config regen
# add other plugins here as needed ^^^
```

You don't need to setup a login and password.

3. Start the Alumet agent to collect measurements.
4. Wait some time (usually < 1min).
5. Visualize the measurements by opening the dashboard at `https://api.grid5000.fr/stable/sites/{SITE}/metrics/dashboard`.
For instance, if your node is in Lyon, go to https://api.grid5000.fr/stable/sites/lyon/metrics/dashboard.
6. Select your node and the metrics you want (such as `rapl_consumed_energy`) on Grafana.
