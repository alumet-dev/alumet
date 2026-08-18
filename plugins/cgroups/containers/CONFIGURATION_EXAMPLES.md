# Container Plugin Configuration Examples

This document provides comprehensive configuration examples for the containers plugin to work with various Docker and Podman setups including WSL2, standard Linux VMs, and bare metal systems.

## Configuration Options

### Basic Configuration

```toml
[[plugins]]
name = "containers"

[plugins.containers]
# Container runtime: "docker" or "podman"
runtime = "docker"

# Optional: custom API URL (supports unix:// and http://)
# If not specified, uses default for the runtime
api_url = "unix:///var/run/docker.sock"

# Optional: explicit Unix socket path (alternative to api_url)
# Example: socket_path = "/var/run/docker.sock"
socket_path = null

# Optional: enable/disable automatic WSL2 socket detection (default: true)
# Set to false if you want to manually specify the socket in WSL2
detect_wsl = true

# Optional: use Windows named pipes (Docker Desktop specific)
use_windows_pipe = false

# Polling interval for container updates
poll_interval = "5s"

# Whether to annotate foreign cgroup measurements
annotate_foreign_measurements = false

# Whether to include container labels as attributes
include_container_labels = false
```

## Environment-Specific Configurations

### 1. Standard Linux VM / Bare Metal - Docker

For standard Linux environments where Docker runs with the default Unix socket:

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
# Uses default: unix:///var/run/docker.sock
```

### 2. Standard Linux VM / Bare Metal - Podman Daemon

For Podman running in daemon mode:

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "podman"
# Uses default: unix:///run/podman/podman.sock
```

### 3. Standard Linux VM / Bare Metal - Podman Daemonless

For Podman running in daemonless mode (rootless), specify user socket:

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "podman"
api_url = "unix:///run/user/1000/podman/podman.sock"
```

### 4. WSL2 with Docker Desktop - Auto Detection

For WSL2 environments with Docker Desktop, enable automatic WSL detection (default):

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
# detect_wsl is enabled by default and will automatically find the socket
# It tries these paths in order:
# 1. //wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock
# 2. //wsl.localhost/docker-desktop/run/docker.sock
# 3. /mnt/wsl/docker-desktop/run/docker.sock
# 4. /mnt/c/Users/Public/docker.sock
# 5. /var/run/docker.sock (fallback)
detect_wsl = true
```

### 5. WSL2 with Docker Desktop - Manual Configuration

If automatic detection fails, explicitly specify the socket path:

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
# Disable automatic detection and use specific socket
detect_wsl = false
api_url = "unix:////wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock"
```

### 6. WSL2 with Docker Desktop - Using socket_path

Alternative approach using the socket_path option:

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
# Use socket_path instead of api_url for unix sockets
socket_path = "//wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock"
```

### 7. WSL2 with Podman

For Podman on WSL2, automatic detection should work:

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "podman"
# detect_wsl is enabled by default
# Will try: /run/podman/podman.sock and /var/run/podman/podman.sock
```

### 8. WSL2 with Podman - Manual Configuration

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "podman"
# Disable auto detection and specify socket
detect_wsl = false
api_url = "unix:///run/podman/podman.sock"
```

### 9. Remote Docker Daemon (SSH Tunnel)

When connecting to a remote Docker daemon via SSH tunnel:

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
api_url = "http://localhost:2375"
# Assuming: ssh -L 2375:/var/run/docker.sock remote-host
```

### 10. Docker with Custom Socket Path

For Docker with non-standard socket location:

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
api_url = "unix:///custom/path/docker.sock"
```

### 11. Podman with Rootless Mode

For Podman running in rootless mode with custom user:

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "podman"
api_url = "unix:///run/user/1001/podman/podman.sock"
# Replace 1001 with your actual UID
```

## All Possible Socket Paths

### Docker Socket Paths

**Standard Linux:**
- `unix:///var/run/docker.sock` (default)
- `unix:///run/docker.sock`

**WSL2 with Docker Desktop:**
- `unix:////wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock`
- `unix:///mnt/wsl/docker-desktop/run/docker.sock`
- `unix:///var/run/docker.sock`

**Custom configurations:**
- `unix:///custom/path/docker.sock`

**Remote/Network:**
- `http://localhost:2375` (requires -H tcp://0.0.0.0:2375)
- `http://remote-host:2375`

### Podman Socket Paths

**Standard daemon mode:**
- `unix:///run/podman/podman.sock` (default)
- `unix:///var/run/podman/podman.sock`

**Rootless daemonless mode:**
- `unix:///run/user/<uid>/podman/podman.sock`
- `unix:///run/user/1000/podman/podman.sock` (for UID 1000)

**Rootless daemon mode:**
- `unix:///run/user/<uid>/podman/podman.sock`
- `unix:XDG_RUNTIME_DIR/podman/podman.sock`

**Network:**
- `http://localhost:8080` (requires podman system service)
- `http://remote-host:8080`

## Troubleshooting

### Check if socket exists

```bash
# For Docker
ls -la /var/run/docker.sock

# For Podman
ls -la /run/podman/podman.sock

# Check WSL2 specific paths
ls -la /mnt/wsl/docker-desktop/run/docker.sock
```

### Test socket connectivity

```bash
# Test Docker
curl --unix-socket /var/run/docker.sock http://localhost/_ping

# Test Podman
curl --unix-socket /run/podman/podman.sock http://localhost/_ping
```

### WSL2 Specific Issues

If encountering WSL2 issues:

1. **Ensure Docker Desktop is running** in Windows
2. **Check WSL2 integration** in Docker Desktop settings
3. **Try different socket paths** mentioned in WSL2 examples
4. **Check WSL2 version**: `wsl --list --verbose`

### Permission Issues

```bash
# Add user to docker group (Linux)
sudo usermod -aG docker $USER

# For Podman, ensure proper permissions
sudo chmod 666 /run/podman/podman.sock
```

## Advanced Configuration

### Enable Container Labels

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
include_container_labels = true  # Include labels as attributes
```

### Annotate Foreign Measurements

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
annotate_foreign_measurements = true  # Annotate other plugin measurements
```

### Custom Polling Interval

```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
poll_interval = "10s"  # Poll every 10 seconds
```

## Environment Detection

The plugin includes helper functions that can automatically detect:

- **WSL2 environment**: Checks `/proc/version` for Microsoft strings
- **Podman daemon sockets**: Searches common daemon socket paths
- **Podman rootless sockets**: Searches user namespace sockets

These can be used programmatically to configure the plugin automatically based on the detected environment.

## Summary of Supported Configurations

The containers plugin supports:

✅ **Unix socket URLs** (`unix://path/to/socket`)  
✅ **HTTP URLs** (`http://host:port`)  
✅ **Docker** (standard Linux, WSL2, custom paths)  
✅ **Podman daemon mode** (standard Linux, WSL2)  
✅ **Podman daemonless mode** (rootless, custom users)  
✅ **Remote connections** (via HTTP)  
✅ **Custom socket paths** (for non-standard installations)  
✅ **Automatic environment detection** (WSL2, daemon vs daemonless)

For most users, the default configuration should work. Only specify `api_url` if you have a non-standard setup.