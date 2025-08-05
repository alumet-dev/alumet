# MongoDB plugin

Provides an output to MongoDB.

## Requirements

- host: MongoDB server URL, for example `http://localhost`. You can also use `https`.
- port: Port used by the MongoDB application, for example: `27017`

## Configuration

Here is a configuration example of the plugin. It's part of the Alumet configuration file (eg: `alumet-config.toml`).

```toml
[plugins.mongodb]
# Address of the host where MongoDB is running
host = "localhost"
# Port used by MongoDB
port = "27017"
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
