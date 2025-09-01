# InfluxDB plugin

Provides an output to InfluxDB v2.

## Requirements

- Write access to a running instance of InfluxDB v2.

## Configuration

Here is an example of how to configure this plugin. Put the following in the configuration file of the Alumet agent (usually alumet-config.toml).

```toml
[plugins.influxdb]
# Address of the host where InfluxDB is running
host = "http://localhost:8086"
# Token to write on the database
token = "FILL ME"
/// Organisation where to write data
org = "FILL ME"
/// Bucket where to write data   
bucket = "FILL ME"
# By default, serialize all Alumet attributes as fields. This can be either `"field"` or `"tag".
attributes_as = "field"
# Always serialize the given list of attributes as InfluxDB tags
attributes_as_tags = [""]
# Always serialize the given list of attributes as InfluxDB fields
attributes_as_fields = [""]
```

## More information

### Attribute serialization

InfluxDB does not have "attributes", but "tags" (which are indexed and can only hold strings) and "fields" (which are not indexed and can hold strings, integers, floats and booleans).
For tags, Alumet will automatically serialize the values to strings.

By changing the config options, you can choose which attributes translate to tags and which ones translate to fields.

For example, depending on the config, the same alumet point will lead to different influxdb point. Here is the alumet point's rust structure.

```rust
MeasurementPoint {
    metric: rapl_consumed_energy,
    timestamp: 1755604520429334196,
    value: 123u,
    resource: Resource::CpuPackage { id: 0 },
    consumer: ResourceConsumer::local_machine,
    attributes: [
        {
            domain: "package"
        }
    ]
}
```

#### Example with configuration 1

Serialize all Alumet attributes as fields, here there is `domain`.

```toml
[plugins.influxdb]
# ... <- other entries here (omitted)

# By default, serialize all attributes as fields.
attributes_as = "field"
```

Lead to the following line protocol for influx

```text
# <measurement>[,<tag_key>=<tag_value>[,<tag_key>=<tag_value>]] <field_key>=<field_value>[,<field_key>=<field_value>] [<timestamp>]
rapl_consumed_energy_J,resource_kind=cpu_package,resource_id=0,resource_consumer_kind=local_machine domain="package",value=123u 1755604520429334196
```

#### Example with configuration 2

Serialize all Alumet attributes as fields except `domain`, here there is no attribute concerned.

```toml
[plugins.influxdb]
# ... <- other entries here (omitted)

# By default, serialize all attributes as fields.
attributes_as = "field"
# Except for these attributes, which will become tags.
attributes_as_tags = ["domain"]
```

Lead to the following line protocol for influx

```text
# <measurement>[,<tag_key>=<tag_value>[,<tag_key>=<tag_value>]] <field_key>=<field_value>[,<field_key>=<field_value>] [<timestamp>]
rapl_consumed_energy_J,resource_kind=cpu_package,resource_id=0,resource_consumer_kind=local_machine,domain=package value=123u 1755604520429334196
```

#### Example with configuration 3

Serialize all Alumet attributes as tag except `domain`, here there is no attribute concerned.

```toml
[plugins.influxdb]
# ... <- other entries here (omitted)

# By default, serialize all attributes as tags.
attributes_as = "tag"
# Except for these attributes, which will become fields.
attributes_as_fields = ["domain"]
```

Lead to the following line protocol for influx

```text
# <measurement>[,<tag_key>=<tag_value>[,<tag_key>=<tag_value>]] <field_key>=<field_value>[,<field_key>=<field_value>] [<timestamp>]
rapl_consumed_energy_J,resource_kind=cpu_package,resource_id=0,resource_consumer_kind=local_machine domain="package",value=123u 1755604520429334196
```

### Line protocol influx

You can learn more about the line protocol used in influx v2 [on this web page](https://docs.influxdata.com/influxdb/v2/reference/syntax/line-protocol/)
