# RAPL plugin

The Kwollect plugin is an output plugin. Its aim is to push the metrics to the API of [Grid'5000](https://www.grid5000.fr/w/Grid5000:Home).
This API use Kwollect and let user push their custom metrics as some metrics are already pushed automatically.
The plugin behaviour was inspired by [this documentation](https://www.grid5000.fr/w/Monitoring_Using_Kwollect).

## Requirements

- A node inside Grid'5000
- Metrics from alumet

## Output

Here are the metrics collected by the plugin source.

|Plugin|Name|Blocking|Description|Attributes|More information|
|----|----|----|-----------|
|`kwollect`|`kwollect-output`|`yes`|`Push metrics to the Grid'5000 through url specified in the config`|

## Configuration

Here is a configuration example of the kwollect plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.kwollect]
# Url of the Grid'5000 API, it needs to specify the correct site used
url = "https://api.grid5000.fr/stable/sites/grenoble/metrics"
# Name of the machine
hostname = "mars"
# Login and password used to push the metric, both are optional. If none are specified, it will push using the current user
login = 
password = 
```
