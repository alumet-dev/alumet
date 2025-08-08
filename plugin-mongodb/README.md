# MongoDB plugin

Provides an output to MongoDB.

## Requirements

- A running instance of MongoDb, with version >= 4.0.

## Configuration

Here is a configuration example of the plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.mongodb]
# Address of the host where MongoDB is running
host = "localhost"
# Port used by MongoDB
port = 27017
# Name of the database to use
database = "Belgium"
# Name of the collection within the database
collection = "Books"
# Username and password for authentication purpose
username = "Amelie"
password = "Nothomb"
```

## More information

About attributes, they're all translated into MongoDB fields; if their name is a reserved one, it's formatted as `name_field`.
Here is a example of an MongoDB entry:

```json
{
  "_id": {
    "$oid": "68936f09bfb52feb9d640710"
  },
  "measurement": "cpu_time_delta",
  "resource_kind": "local_machine",
  "resource_id": "5",
  "resource_consumer_kind": "process",
  "resource_consumer_id": "25599",
  "kind": "guest",
  "value": "0u",
  "timestamp": "1754492679656310160",
  "measurement_field": "cpu_time_delta_field"
}
```

In the above example, there is a `measurement` attribute, as it's name is a reserved one, it's translated into `measurement_field`.
The mandatory field `measurement` don't change.
