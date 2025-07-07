# Kwollect-input Plugin

The Kwollect-input plugin creates an Alumet **source** that collects measurements of processor energy usage via [Kwollect](https://gitlab.inria.fr/grid5000/kwollect) on Grid'5000 to get data (at least, power consumption) from it.

## Requirements

- Have an account on Grid'5000.

## Metrics

Here are the metrics collected by the plugin source (if we only take power consumption).

|Timestamp|Device-id|Metric-id|Value|Labels|
|----|----|----|-----------|----------|
|`2025-06-20T14:15:20.005984+02:00`|taurus-7|wattmetre_power_watt|131.7|{"_device_orig":["wattmetre1-port6"]}||

## Attributes

### Configuration

Here is a configuration example of the Kwollect-input plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.kwollect-input]
site = "lyon"
hostname = "taurus-7"
metrics = "wattmetre_power_watt"
login = "login"
password = "password"
```

### Usage

```bash
$ alumet-agent --plugins kwollect-input ... # you can add run, exec, or other plugins if you want to
```

### More information