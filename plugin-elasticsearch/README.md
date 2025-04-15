# ElasticSearch / OpenSearch plugin

The `elasticsearch` plugin inserts Alumet measurements into ElasticSearch or OpenSearch (the API that we use is identical in both projects).

## Configuration

The default configuration is as follows.

```toml
[plugins.elasticsearch]
server_url = "http://localhost:9200"
allow_insecure = false
index_prefix = "alumet"
metric_unit_as_index_suffix = false

[plugins.elasticsearch.auth.basic]
user = "TODO"
password = "TODO"
```

### Authentication

Multiple auth schemes are supported.

#### Basic auth

You can give the username and password in the config:

```toml
[plugins.elasticsearch.auth.basic]
user = "TODO"
password = "TODO"
```

Or ask the plugin to read them from a file, which must contain the username and password separated by a colon (`:`):

```toml
[plugins.elasticsearch.auth.basic_file]
file = "basic_auth.txt"
```

Example file `basic_auth.txt`:

```txt
user:password
```

#### API key auth

```toml
[plugins.elasticsearch.auth.api_key]
key = "your key here"
```

#### Bearer auth

```toml
[plugins.elasticsearch.auth.bearer]
token = "your token here"
```
