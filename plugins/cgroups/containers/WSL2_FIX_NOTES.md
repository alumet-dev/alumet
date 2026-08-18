# WSL2 Docker Socket Connection Fix

## Problem

The containers plugin was failing to connect to Docker sockets in WSL2 environments with the error:
```
URL scheme is not allowed
```

This occurred because the `reqwest` HTTP client doesn't natively support `unix://` URL schemes, and the socket detection logic for WSL2 was incomplete.

## Solution

The following changes have been implemented to fix WSL2 and improve support for various container runtime configurations:

### 1. Enhanced WSL2 Socket Detection

The `detect_wsl_socket_path` function in `client.rs` now checks multiple WSL2-specific socket paths in order of priority:

**Docker Desktop in WSL2:**
- `//wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock` (primary)
- `//wsl.localhost/docker-desktop/run/docker.sock` (alternative)
- `/mnt/wsl/docker-desktop/run/docker.sock` (legacy)
- `/mnt/c/Users/Public/docker.sock` (Windows mount)
- `/var/run/docker.sock` (fallback)

**Podman in WSL2:**
- `/run/podman/podman.sock` (standard)
- `/var/run/podman/podman.sock` (alternative)

### 2. New Configuration Options

Added three new configuration fields in `config.rs`:

```toml
[plugins.containers]
runtime = "docker"

# New options
socket_path = null                    # Alternative to api_url for unix sockets
detect_wsl = true                     # Enable/disable automatic WSL detection (default)
use_windows_pipe = false              # Windows named pipes (Docker Desktop)
```

### 3. Improved Socket Resolution Logic

The plugin now follows this priority order for determining the socket path:

1. **Explicit `socket_path`** if specified
2. **Automatic WSL2 detection** if `detect_wsl = true`
3. **Configured `api_url`** if specified  
4. **Runtime default** as fallback

### 4. Better Error Messages

Error messages now include specific WSL2 troubleshooting suggestions:
```
failed to create API client for docker at unix:///var/run/docker.sock. 
If running in WSL2, try setting api_url to 
'unix:////wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock' 
or specify socket_path in configuration.
```

## Usage

### For WSL2 with Docker Desktop (Recommended)

**Automatic Detection** (works in most cases):
```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
# detect_wsl = true is the default
```

**Manual Configuration** (if auto-detection fails):
```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
detect_wsl = false
api_url = "unix:////wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock"
```

**Using socket_path**:
```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
socket_path = "//wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock"
```

### For Standard Linux/Bare Metal

**Docker** (default configuration):
```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "docker"
# Uses unix:///var/run/docker.sock by default
```

**Podman Daemon**:
```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "podman"
# Uses unix:///run/podman/podman.sock by default
```

**Podman Rootless**:
```toml
[[plugins]]
name = "containers"

[plugins.containers]
runtime = "podman"
api_url = "unix:///run/user/1000/podman/podman.sock"
```

## All Supported Configurations

The plugin now supports:

✅ **WSL2 with Docker Desktop** (automatic and manual)
✅ **WSL2 with Podman** (automatic and manual)  
✅ **Standard Linux VMs / Bare Metal**
✅ **Docker** (default and custom sockets)
✅ **Podman daemon mode**
✅ **Podman daemonless/rootless mode**
✅ **Remote connections** (HTTP/TCP)
✅ **Custom socket paths**
✅ **Windows named pipes** (future support)

## Technical Details

### Why the Original Error Occurred

The `reqwest` crate doesn't support `unix://` URL schemes out of the box. When the plugin tried to create an HTTP client with a `unix://` URL, reqwest rejected it as an invalid URL scheme.

### How It Was Fixed

The existing code already had logic to handle Unix sockets via `curl`, but:

1. **Wrong socket paths** for WSL2 - used `/mnt/wsl/...` which doesn't always work
2. **Missing primary WSL2 path** - `//wsl.localhost/...` which is the correct path
3. **No fallback logic** - didn't try multiple paths
4. **Poor error messages** - didn't suggest WSL2 solutions

The solution:
- Use `curl` for Unix socket connections (already implemented)
- Add proper WSL2 socket detection with multiple fallback paths
- Provide flexible configuration options
- Improve error messages and documentation

### Unix Socket Integration

For Unix socket connections, the plugin uses standard Rust `UnixStream` from `std::os::unix::net` to communicate directly with Docker/Podman sockets. This creates a pure Rust HTTP client that:

1. **No external dependencies** - Uses only standard Rust libraries
2. **No CLI requirements** - Doesn't depend on `curl` being installed
3. **Better error handling** - Proper Rust error context and types
4. **Cross-platform** - Works consistently across Unix-like systems

The implementation:
- Connects to Unix sockets using `UnixStream::connect()`
- Constructs raw HTTP/1.1 requests manually
- Parses HTTP responses to extract JSON data
- Works identically to `curl --unix-socket` command

Example HTTP request sent to socket:
```http
GET /containers/json?all=true HTTP/1.1
Host: localhost
Connection: close

```

## Troubleshooting

### WSL2 Issues

1. **Check if Docker Desktop is running** in Windows
2. **Verify WSL2 integration** is enabled in Docker Desktop settings
3. **Test socket connectivity** manually:
   ```bash
   curl --unix-socket //wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock http://localhost/_ping
   ```
4. **Try different socket paths** from the documentation
5. **Check WSL2 version**: `wsl --list --verbose`

### Permission Issues

```bash
# Add user to docker group (Linux)
sudo usermod -aG docker $USER
newgrp docker

# For Podman, ensure proper permissions
sudo chmod 666 /run/podman/podman.sock
```

### Verify Socket Exists

```bash
# Standard Linux
ls -la /var/run/docker.sock

# WSL2
ls -la //wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock

# Podman
ls -la /run/podman/podman.sock
```

## Testing

Test your configuration before running the full plugin:

```bash
# Test Docker connection
curl --unix-socket <your-socket-path> http://localhost/_ping

# Test container listing
curl --unix-socket <your-socket-path> http://localhost/containers/json?all=true
```

For WSL2 with Docker Desktop:
```bash
curl --unix-socket //wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock http://localhost/containers/json?all=true
```

## References

- **Configuration Examples**: See `CONFIGURATION_EXAMPLES.md` for detailed examples
- **Original Issue**: URL scheme not allowed error with unix:// URLs in WSL2
- **Key Files Modified**:
  - `src/client.rs` - Enhanced WSL2 detection
  - `src/config.rs` - New configuration options
  - `src/lib.rs` - Improved socket resolution logic
  - `CONFIGURATION_EXAMPLES.md` - Comprehensive documentation

## Support

For issues or questions:
1. Check the troubleshooting section above
2. Review `CONFIGURATION_EXAMPLES.md` for your specific scenario
3. Test socket connectivity manually using curl
4. Verify your configuration matches the examples