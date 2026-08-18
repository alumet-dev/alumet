# Containers Plugin

A plugin for Alumet that provides annotations for Docker and Podman container cgroups.

## Overview

The `containers` plugin measures resources consumed by Docker and Podman containers by annotating cgroup measurements with container metadata. It can annotate both its own cgroup measurements and measurements from other plugins (like the raw cgroup plugin).

## Features

- **Multi-Runtime Support**: Works with both Docker and Podman (configured via TOML)
- **Automatic Detection**: Automatically detects containers from cgroup paths
- **Flexible Annotation**: Can annotate measurements from any cgroup source
- **Container Labels**: Optional support for including container labels as attributes
- **Efficient Caching**: Implements caching to minimize API calls

## Annotations Provided

When enabled, the plugin adds the following attributes to cgroup measurements:

| Attribute | Type | Description |
|-----------|------|-------------|
| `container_id` | string | Container unique identifier |
| `container_name` | string | Container name |
| `container_image` | string | Image name used to create the container |
| `runtime` | string | Either "docker" or "podman" |
| `container_created` | i64 | Container creation timestamp (optional) |
| `label.{key}` | string | Container labels (if `include_container_labels` is enabled) |

## Configuration

Add the plugin to your Alumet configuration file (`alumet.toml`):

```toml
[[plugins]]
name = "containers"

[plugins.containers]
# Container runtime to use: "docker" or "podman"
runtime = "docker"

# URL to the container API (default depends on runtime)
# Docker: unix:///var/run/docker.sock
# Podman: unix:///run/podman/podman.sock
# You can also use HTTP endpoints like: http://localhost:2375
api_url = null

# How often to refresh the container list
poll_interval = "5s"

# Whether to annotate measurements from other plugins
# When true, the plugin will add container attributes to all cgroup measurements
annotate_foreign_measurements = true

# Whether to include container labels as attributes
# Labels will be prefixed with "label." (e.g., "label.com.example.key")
include_container_labels = false
```

### Example Configuration for Docker

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
annotate_foreign_measurements = true
include_container_labels = true
```

### Example Configuration for Podman

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "podman"
api_url = "unix:///run/podman/podman.sock"
annotate_foreign_measurements = true
```

## Usage

The plugin automatically detects containers from their cgroup paths and annotates measurements. Supported cgroup path patterns include:

### Docker
- `/sys/fs/cgroup/docker/<container_id>/`
- `/sys/fs/cgroup/buildkit/<container_id>/`
- `/sys/fs/cgroup/system.slice/docker-<container_id>.scope`

### Podman
- `/sys/fs/cgroup/libpod_parent/<container_id>/`
- `/sys/fs/cgroup/user.slice/libpod_parent/<container_id>/`
- `/sys/fs/cgroup/user.slice/libpod-<container_id>.scope`

## Integration with Other Plugins

The plugin is designed to work seamlessly with other cgroup plugins:

1. **Raw Cgroup Plugin**: The containers plugin can annotate measurements from the raw cgroup plugin
2. **Kubernetes Plugin**: Can be used alongside K8S monitoring for workloads not managed by Kubernetes
3. **HPC Plugins**: Can complement OAR and SLURM plugins for containerized HPC workloads

## Requirements

- Linux cgroups v2 (recommended) or v1
- Docker or Podman running and accessible via their API
- Appropriate permissions to access the container API socket

## Troubleshooting

### "failed to list containers" Error

This error typically occurs when the plugin cannot connect to the container API. Check:

1. **API URL**: Ensure the `api_url` configuration is correct for your setup
2. **Permissions**: Ensure the user running Alumet has permission to access the container socket
3. **Runtime**: Ensure Docker or Podman is running

### No Containers Found

If the plugin starts successfully but doesn't find containers:

1. **Check Running Containers**: Verify that containers are actually running (`docker ps` or `podman ps`)
2. **Cgroup Path**: Check that containers are being placed in the expected cgroup paths
3. **Runtime Version**: Ensure you're using a supported version of Docker or Podman

### Missing Annotations

If measurements don't have container annotations:

1. **Check Configuration**: Ensure `annotate_foreign_measurements` is set to `true`
2. **Cgroup Version**: Annotation transforms require cgroup v2 (cgroup v1 is not currently supported)
3. **Path Matching**: Verify that your containers use the expected cgroup path patterns

## Architecture

The plugin follows the same architecture as other cgroup plugins in Alumet:

- **API Client**: Handles communication with Docker/Podman APIs
- **Container Registry**: Maintains a cache of container information
- **ID Extraction**: Parses cgroup paths to extract container IDs
- **JobTagger**: Implements the trait for adding container attributes

## Contributing

This plugin is part of the Alumet project. Contributions are welcome!

## License

This plugin follows the same license as the main Alumet project.