# InfluxDB plugin

Provides an output to InfluxDB 2.

## Config options

- host: InfluxDB server URL, for example `http://localhost:8086`. You can also use `https`.
- token: your authentication token
- org: organization to write the measurements to
- bucket: bucket to write the measurements to
- attribute_as: how to serialize the Alumet attributes. This can be either `"field"` or `"tag"`.
- attribute_as_tags (optional): always serialize the given list of attributes as InfluxDB tags
- attribute_as_fields (optional): always serialize the given list of attributes as InfluxDB fields

## Attribute serialization

InfluxDB does not have "attributes", but "tags" (which are indexed and can only hold strings) and "fields" (which are not indexed and can hold strings, integers, floats and booleans).

By changing the config options, you can choose which attributes translate to tags and which ones translate to fields. Here is an example (incomplete config):

```toml
[plugins.influxdb]
# ... <- other entries here (omitted)

# By default, serialize all attributes as fields.
attributes_as = "field"
# Except for these attributes, which will become tags.
attributes_as_tags = ["domain"]
```

For tags, Alumet will automatically serialize the values to strings.
