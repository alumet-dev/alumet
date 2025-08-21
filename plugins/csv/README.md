# CSV plugin

Provides an output to CSV.

## Requirements

- Write permissions to the csv file

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`)

```toml
[plugins.csv]
# Absolute or relative path to the output_file
output_path = "alumet-output.csv"
# Do we flush after each write (measurements)?
force_flush = true
# Do we append the unit (unique name) to the metric name?
append_unit_to_metric_name = true
# Do we use the unit display name (instead of its unique name)?
use_unit_display_name = true
# The CSV delimiter, such as `;`
csv_delimiter = ";"
```

## More information

### Format of the output file

|metric|timestamp|value|resource_kind|resource_id|consumer_kind|consumer_id|(attribute_1)|(...)|__late_attributes|
|------|---------|-----|-------------|-----------|-------------|-----------|-------------|-----|-----------------|
|Metric in format {metric_name}_{unit} See [Example](#example-metric-format)|Time in format [rfc3339](https://www.rfc-editor.org/rfc/rfc3339.html)|The measured value|See Enum [Resource](https://docs.rs/alumet/latest/alumet/resources/enum.Resource.html)|See Enum [Resource](https://docs.rs/alumet/latest/alumet/resources/enum.Resource.html)|See Enum [ResourceConsumer](https://docs.rs/alumet/latest/alumet/resources/enum.ResourceConsumer.html)|See Enum [ResourceConsumer](https://docs.rs/alumet/latest/alumet/resources/enum.ResourceConsumer.html)|Additional attributes for the metric|One per column|Additional attributes in format {name}={value}|

See [MeasurementPoint](https://docs.rs/alumet/latest/alumet/measurement/struct.MeasurementPoint.html) for more details

#### Example metric format

The optional unit is in the form of its unique name or displaying name as specified by the Unified Code for Units of Measure (UCUM).

- `memory_usage` (metric name no unit)
- `memory_usage_B` (metric name and unit as display_name)
- `memory_usage_By` (metric name and unit as unique_name)

### Output example with late_attributes

The late_attributes is used for attributes that arrive to the CSV output after the header has already been written to the file.

```csv
metric,timestamp,value,resource_kind,resource_id,consumer_kind,consumer_id,__late_attributes
cpu_time_delta_nanos,2025-01-01T12:00:00.000000000Z,1720000000,local_machine,,process,15,kind=user
```

### Output example with additional attributes

```csv
metric,timestamp,value,resource_kind,resource_id,consumer_kind,consumer_id,name,namespace,node,uid,__late_attributes
cpu_time_delta_nanos,2025-01-01T12:00:00.000000000Z,1720000000,local_machine,,cgroup,kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-pod00b506dc-87ee-462c-880d-3e41d0dacd0c.slice,pod1,default,test-node,00b506dc-87ee-462c-880d-3e41d0dacd0c,,
```
