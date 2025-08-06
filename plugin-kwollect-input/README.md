# Kwollect-input Plugin

The **Kwollect-input** plugin creates a **source** in Alumet that collects processor energy usage measurements via [Kwollect](https://gitlab.inria.fr/grid5000/kwollect) on the Grid’5000 platform.
Currently, it mainly gathers power consumption data (in watts) on only one node at a time.

## Requirements

- You must have an account on Grid’5000.
- You want to collect Kwollect data, specifically wattmeter measurements, on a node.

The clusters & nodes that support wattmeter are as follows:

- **grenoble**: servan, troll, yeti
- **lille**: chiroptera
- **lyon**: gemini, neowise, nova, orion, pyxis, sagittaire, sirius, taurus
- **nancy**: gros⁺
- **rennes**: paradoxe

## Example Metrics Collected

Here is an example of the metrics collected by the plugin:

| Metric | Timestamp | Value (W) | Resource Type | Resource ID | Consumer Type | Consumer ID | Metric ID |
|--------|-----------|-----------|---------------|-------------|---------------|-------------|-----------|
| wattmetre_power_watt_W | 2025-07-22T08:28:12.657Z | 129.69 | device_id | taurus-7 | device_origin | wattmetre1-port6 | wattmetre_power_watt |
| wattmetre_power_watt_W | 2025-07-22T08:28:12.657Z | 128.80 | device_id | taurus-7 | device_origin | wattmetre1-port6 | wattmetre_power_watt |
| ... | ... | ... | ... | ... | ... | ... | ... |

Each entry represents a power measurement in watts, with a precise timestamp, node name (e.g., "taurus-7"), and device identifier (e.g., "wattmetre1-port6").

## Configuration

Here is a configuration example of the plugin. It's part of the Alumet configuration file (e.g., alumet-config.toml):

```toml
[plugins.kwollect-input]
site = "lyon"                     # Grid'5000 site
hostname = "taurus-7"             # Target node hostname
metrics = "wattmetre_power_watt"  # Metric to collect, DO NOT CHANGE IT
login = "YOUR_G5K_LOGIN"          # Your Grid'5000 username
password = "YOUR_G5K_PASSWORD"    # Your Grid'5000 password
```

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

Here’s an excerpt from the logs showing that the API is called successfully:

```text
...
[2025-08-05T07:44:46Z INFO  alumet::agent::exec] Child process exited with status exit status: 0, Alumet will now stop.
[2025-08-05T07:44:46Z INFO  alumet::agent::exec] Publishing EndConsumerMeasurement event
[2025-08-05T07:44:46Z INFO  plugin_kwollect_input] API request should be triggered with URL: https://api.grid5000.fr/stable/sites/lyon/metrics?nodes=taurus-7&metrics=wattmetre_power_watt&start_time=1754379876&end_time=1754379886
[2025-08-05T07:44:46Z INFO  plugin_kwollect_input::source] Polling KwollectSource
[2025-08-05T07:44:48Z INFO  alumet::agent::builder] Stopping the plugins...
...
```

## Some advice

- Verify the Kwollect API is active for your node and not under maintenance with the [status tool](https://www.grid5000.fr/status/).
- Verify if the wattmeters work on the node you want to use by looking at the API URL with the time format `year-month-dayThour:minutes:seconds`:
`https://api.grid5000.fr/stable/sites/{site}/metrics?nodes={node}&start_time={now}&end_time={at least +1s}`

## License

Copyright 2025 Marie-Line DA COSTA BENTO.

Alumet project is licensed under the European Union Public Licence (EUPL). See the [LICENSE](https://github.com/alumet-dev/alumet/blob/main/LICENSE) file for more details.

## More Information

For further details, please check the [Kwollect documentation](https://www.grid5000.fr/w/Monitoring_Using_Kwollect).
