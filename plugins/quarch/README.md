# Quarch Plugin

This plugin measures disk power consumption using a Quarch Power Analysis Module.

## Requirements

### Hardware

1. A Quarch Power Analysis Module 
2. If you want to use it on Grid'5000:
    - Have an account on Grid'5000.
    - Use a Grenoble node (Quarch module is physically installed there).

### Software
- A working quarchpy installation (Python package)
- A Java runtime (configured in `java_bin`, should be installed by quarchpy).

## Metrics

The plugin exposes the following metric:

| metric | timestamp | value | resource_kind | resource_id | consumer_kind | consumer_id | __late_attributes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| disk_power_W | 2025-09-01T10:45:41.757250914Z | 9.526866534 | local_machine | | local_machine | | |
| disk_power_W | 2025-09-01T10:45:42.723658463Z | 9.526885365 | local_machine | | local_machine | | |
| disk_power_W | 2025-09-01T10:45:43.723659913Z | 9.528410676 | local_machine | | local_machine | | |
| disk_power_W | 2025-09-01T10:45:44.723650353Z | 9.528114186 | local_machine | | local_machine | | |

*Meaning*:
- `disk_power_W `= instantaneous power consumption of the disk in Watts.
- Sampling rate is controlled via the plugin configuration (`sample`, `poll_interval`).

## Configuration

Here is a configuration example of the plugin. It's part of the Alumet configuration file (e.g., `alumet-config.toml`).

```toml
[plugins.quarch]

# --- Quarch connection settings ---
quarch_ip = "CHANGE HERE"       # a.g.,"172.17.30.102" for Grenoble G5K
quarch_port = 9760              # Default if unchangedon your module
qis_port = 9780                 # Default if unchanged on your module
java_bin = "path_to_java"       # Installed with quarchpy: ".../lib/python3.11/site-packages/quarchpy/connection_specific/jdk_jres/lin_amd64_jdk_jre/bin/java"
qis_jar_path = "path_to_qis"    # Installed with quarchpy: ".../lib/python3.11/site-packages/quarchpy/connection_specific/QPS/win-amd64/qis/qis.jar"

# --- Measurement settings ---
poll_interval = "150ms"            # Interval between two reported measurements
flush_interval = "1500ms"           # Interval between flushing buffered data
```

*Notes:*
- `poll_interval` controls how often Alumet queries the Quarch module.
- `flush_interval` controls how often buffered measurements are sent downstream.
- Ensure `java_bin` and `qis_jar_path` are correct (installed with quarchpy).

### Recommended `poll_interval` and `flush_interval`
``` bash
| sample (2^n) | ~Hardware Window | `poll_interval` (recommended) | `flush_interval` (recommended)|
| ------------ | ---------------------------- | -------------------------- | --------------------------- |
| 32           | 0.13 ms                      | 200 µs                     | 2 ms                        |
| 64           | 0.25 ms                      | 300 µs                     | 3 ms                        |
| 128          | 0.5 ms                       | 500 µs                     | 5 ms                        |
| 256          | 1 ms                         | 1 ms                       | 10 ms                       |
| 512          | 2 ms                         | 2 ms                       | 20 ms                       |
| 1K (1024)    | 4.1 ms                       | 5 ms                       | 50 ms                       |
| 2K (2048)    | 8.2 ms                       | 10 ms                      | 100 ms                      |
| 4K (4096)    | 16.4 ms                      | 20 ms                      | 200 ms                      |
| 8K (8192)    | 32.8 ms                      | 50 ms                      | 500 ms                      |
| 16K (16384)  | 65.5 ms                      | 100 ms                     | 1 s                         |
| 32K (32768)  | 131 ms                       | 150 ms                     | 1500 ms                     |
```

*Notes:*
- Choosing `poll_interval` < `hardware window` (min 0.13 ms) may result in repeated identical readings.
- Choosing `poll_interval` > `hardware window` (max 131 ms) may skip some module measurements, which is acceptable depending on your experiment duration. *For example, if you want 1 poll per second, `poll_interval`= 1s will work.*

## Usage

### Virtual environment (recommended)

To isolate `quarchpy`, create a Python virtual environment:

``` bash
$ python3 -m venv /root/<Name_Virtual_Environnement> && \
/root/<Name_Virtual_Environnement>/bin/pip install --upgrade pip && \
/root/<Name_Virtual_Environnement>/bin/pip install --upgrade quarchpy
$ source /root/<Name_Virtual_Environnement>/bin/activate
```

### Commands

```bash
# Run a command while measuring disk power
$ alumet-agent --plugins quarch exec <COMMAND_TO_EXEC>

# Run alumet with continuous measurements
$ alumet-agent --plugins quarch run

# Save results to CSV (with another plugin)
$ alumet-agent --output-file "measurements-quarch.csv" --plugins quarch,csv run
```

### Usage on Grid'5000

- The Quarch Module is physically installed on yeti-4 (Grenoble).
- You can access it from any Grenoble node.
- Example configuration for G5K:
```bash
quarch_ip = "172.17.30.102"
quarch_port = 9760
qis_port = 9780
```

### Outputs examples

```bash
...
[2025-09-01T10:45:40Z INFO  alumet::agent::builder] Plugin startup complete.
    🧩 1 plugins started:
        - quarch v0.1.0

    ⭕ 24 plugins disabled: ...
    📏 1 metric registered:
        - disk_power: F64 (W)
    📥 1 source, 🔀 0 transform and 📝 0 output registered.
...
```

### Troubleshooting

- No metrics appear: check `quarch_ip / ports`, and ensure module is powered on.
- Java errors: verify `java_bin` path from your quarchpy install.
- QIS not found: update `qis_jar_path` to the correct installed JAR.

## License

Copyright 2025 Marie-Line DA COSTA BENTO.

Alumet project is licensed under the European Union Public Licence (EUPL). See the [LICENSE](https://github.com/alumet-dev/alumet/blob/main/LICENSE) file for more details.

## More information

Quarch module commands are based on the [SCPI](https://www.ivifoundation.org/specifications/default.html) specification.
For further details, please check the [Quarch Github](https://github.com/QuarchTechnologyLtd).

