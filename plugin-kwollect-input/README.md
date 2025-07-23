# Kwollect-input Plugin

The **Kwollect-input** plugin creates a **source** in Alumet that collects processor energy usage measurements via [Kwollect](https://gitlab.inria.fr/grid5000/kwollect) on the Grid’5000 platform.  
Currently, it mainly gathers power consumption data (in watts) on only one node at a time.

## Requirements

- You must have an account on Grid’5000.  
- You want to collect Kwollect data, specifically wattmeter measurements, on a node.

The clusters & nodes that supports wattmeter are these ones:
- **grenoble**: servan, troll, yeti, wattmetre1, wattmetre2
- **lille**: chirop, wattmetrev3-1
- **lyon**: gemini, neowise, nova, orion, pyxis, sagittaire, sirius, taurus, wattmetre1, wattmetrev3-1, wattmetrev3-2
- **nancy**: gros⁺, gros-wattmetre2
- **rennes**: paradoxe, wattmetrev3-1

## Example Metrics Collected

Here is an example of the metrics collected by the plugin:

| Metric                   | Timestamp                      | Value (W)    | Resource Type | Resource ID  | Consumer Type   | Consumer ID        | Metric ID              |
| ------------------------ | ------------------------------ | ------------ | ------------- | ------------ | --------------- | ------------------ | ---------------------- |
| wattmetre_power_watt_W   | 2025-07-22T08:28:12.657Z       | 129.69       | device_id     | taurus-7     | device_origin   | wattmetre1-port6   | wattmetre_power_watt   |
| wattmetre_power_watt_W   | 2025-07-22T08:28:12.657Z       | 128.80       | device_id     | taurus-7     | device_origin   | wattmetre1-port6   | wattmetre_power_watt   |
| ...                      | ...                            | ...          | ...           | ...          | ...             | ...                | ...                    |

Each entry represents a power measurement in watts, with a precise timestamp, node name (e.g., "taurus-7"), and device identifier (e.g., "wattmetre1-port6").

## Plugin Configuration

Example configuration for the Kwollect-input plugin in Alumet’s config file (`alumet-config.toml`):

```toml
[plugins.kwollect-input]
site = "lyon"                     # Grid'5000 site
hostname = "taurus-7"             # Target node hostname
metrics = "wattmetre_power_watt"  # Metric to collect, DO NOT CHANGE IT
login = "YOUR_G5K_LOGIN"          # Your Grid'5000 username
password = "YOUR_G5K_PASSWORD"    # Your Grid'5000 password
````

## Usage

To run Alumet with this plugin, use:

```bash
alumet-agent --plugins kwollect-input exec ...
```

You can add other plugins as needed, for example to save data to a CSV file:

```bash
alumet-agent --output-file "measurements-kwollect.csv" --plugins csv,kwollect-input exec ...
```

## Example Output

Here’s an excerpt from the logs showing that measurements are being collected successfully:

```text
[2025-07-22T11:05:25Z INFO  alumet_agent] Starting Alumet agent 'alumet-agent' v0.8.4-528f57b...
[2025-07-22T11:05:25Z INFO  plugin_kwollect_input] Kwollect-input plugin is starting
...
[2025-07-22T11:05:35Z INFO  plugin_kwollect_input::source] Parsed measurements: [
  { "timestamp": "2025-07-22T13:05:25+02:00", "device_id": "taurus-7", "metric_id": "wattmetre_power_watt", "value": 90.18, "labels": {"_device_orig": ["wattmetre1-port6"]} },
  ...
]
```

## More Information

For further details, please check the [Kwollect documentation](https://www.grid5000.fr/w/Monitoring_Using_Kwollect).
