# ElasticSearch / OpenSearch plugin

The `elasticsearch` plugin inserts Alumet measurements into ElasticSearch or OpenSearch (the API that we use is identical in both projects).

## Requirements

- Write access to an ElasticSearch or Opensearch instance

## Configuration

Here is an example of how to configure this plugin.
Put the following in the configuration file of the Alumet agent (usually `alumet-config.toml`).

```toml
[plugins.elasticsearch]
# The url of the database instance.
server_url = "http://localhost:9200"
# Controls the use of certificate validation and hostname verification.
# You should think very carefully before you use this!
allow_insecure = false
# The prefix added to each index (format `{index_prefix}-{metric_name}`).
index_prefix = "alumet"
# Controls the use of an optional suffix for each index (format `{index_prefix}-{metric_name}-{metric_unit_unique_name}`).
metric_unit_as_index_suffix = false

[plugins.elasticsearch.auth.basic]
# Authentication Settings: Credentials in the config (Basic auth)
user = "TODO"
password = "TODO"
```

### Authentication

Multiple auth schemes are supported.

#### Basic auth

You can give the username and password in the config:

```toml
[plugins.elasticsearch.auth.basic]
# Authentication Settings: Credentials in the config (Basic auth)
user = "TODO"
password = "TODO"
```

Or ask the plugin to read them from a file, which must contain the username and password separated by a colon (`:`):

```toml
[plugins.elasticsearch.auth.basic_file]
# Authentication Settings: Credentials in another file (Basic auth)
file = "basic_auth.txt"
```

Example file `basic_auth.txt`:

```txt
user:password
```

#### API key auth

```toml
[plugins.elasticsearch.auth.api_key]
# Authentication Settings: Credentials in the config (API key auth)
key = "your key here"
```

#### Bearer auth

```toml
[plugins.elasticsearch.auth.bearer]
# Authentication Settings: Credentials in the config (Bearer auth)
token = "your token here"
```

## More information

The `elasticsearch` plugin inserts Alumet measurements by generating an `index` in the database for each metric in format `{index_prefix}-{metric_name}` or `{index_prefix}-{metric_name}-{metric_unit_unique_name}` if configured so.

See [index basics](https://www.elastic.co/docs/manage-data/data-store/index-basics).

### Output Example

Here is the json representation for a `MeasurementPoint` for the metric `kernel_cpu_time` inside the database:

```json
{
  "_index": "alumet-kernel_cpu_time",
  "_id": "gmyozJgBNhZm2PkkYwJ8",
  "_version": 1,
  "_score": 1,
  "fields": {
    "cpu_state": [
      "user"
    ],
    "consumer_id": [
      ""
    ],
    "@timestamp": [
      "2025-01-01T12:00:00.000000000Z"
    ],
    "cpu_state.keyword": [
      "user"
    ],
    "resource_id": [
      ""
    ],
    "resource_kind": [
      "local_machine"
    ],
    "consumer_kind": [
      "local_machine"
    ],
    "value": [
      1420
    ]
  }
}
```

See [MeasurementPoint](https://docs.rs/alumet/latest/alumet/measurement/struct.MeasurementPoint.html) for more details.
