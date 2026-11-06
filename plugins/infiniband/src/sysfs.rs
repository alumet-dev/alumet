//! Low-level access to the InfiniBand counters exposed by sysfs.

use std::{fmt, fs, io::Read, path::Path};

use anyhow::{Context, anyhow};

/// Default root of the InfiniBand sysfs hierarchy.
pub const DEFAULT_ROOT: &str = "/sys/class/infiniband";

/// Identifies one port of one InfiniBand device.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortKey {
    pub device: String,
    pub port: u32,
}

impl fmt::Display for PortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.device, self.port)
    }
}

/// Restricts which devices and ports are discovered.
///
/// An empty vector means "no restriction", i.e. all devices or all ports.
#[derive(Debug, Clone, Default)]
pub struct Filters {
    pub devices: Vec<String>,
    pub ports: Vec<u32>,
}

/// Raw counters read from sysfs.
///
/// `rcv_data` and `xmit_data` are expressed in 4-byte units, as specified by
/// the InfiniBand sysfs interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawCounters {
    pub rcv_packets: u64,
    pub xmit_packets: u64,
    pub rcv_data: u64,
    pub xmit_data: u64,
}

/// Discovers the ports under `root`, applies `filters`, and returns them
/// sorted by `(device, port)`.
///
/// # Errors
///
/// Returns an error if `root` or a `<device>/ports` directory cannot be read.
/// Individual unreadable entries are skipped with a debug log.
pub fn discover_ports(root: &Path, filters: &Filters) -> anyhow::Result<Vec<PortKey>> {
    let device_entries =
        fs::read_dir(root).with_context(|| format!("failed to read InfiniBand root {}", root.display()))?;

    let mut ports = Vec::new();
    for device_entry in device_entries {
        let device_entry = match device_entry {
            Ok(entry) => entry,
            Err(e) => {
                log::debug!("skipping unreadable entry of {}: {e:#}", root.display());
                continue;
            }
        };
        let path = device_entry.path();
        // Device entries in sysfs are symlinks to the real device directory, so
        // follow them instead of using DirEntry::file_type (which does not).
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(e) => {
                log::debug!("skipping {}: cannot get metadata: {e:#}", path.display());
                continue;
            }
        };
        if !metadata.is_dir() {
            continue;
        }

        let name = device_entry.file_name();
        let Some(device) = name.to_str() else {
            log::debug!("skipping non-UTF-8 device entry in {}", root.display());
            continue;
        };
        if !filters.devices.is_empty() && !filters.devices.iter().any(|d| d == device) {
            continue;
        }
        let device = device.to_owned();

        let ports_dir = path.join("ports");
        let port_entries = match fs::read_dir(&ports_dir) {
            Ok(entries) => entries,
            Err(e) => {
                log::debug!("skipping device {device}: cannot read {}: {e:#}", ports_dir.display());
                continue;
            }
        };

        for port_entry in port_entries {
            let port_entry = match port_entry {
                Ok(entry) => entry,
                Err(e) => {
                    log::debug!("skipping unreadable entry of {}: {e:#}", ports_dir.display());
                    continue;
                }
            };
            let name = port_entry.file_name();
            let Some(name) = name.to_str() else {
                log::debug!("skipping non-UTF-8 port entry in {}", ports_dir.display());
                continue;
            };
            let Ok(port) = name.parse::<u32>() else {
                log::debug!("skipping non-numeric port entry {name:?} in {}", ports_dir.display());
                continue;
            };
            if !filters.ports.is_empty() && !filters.ports.contains(&port) {
                continue;
            }
            ports.push(PortKey {
                device: device.clone(),
                port,
            });
        }
    }

    ports.sort_by(|a, b| a.device.cmp(&b.device).then(a.port.cmp(&b.port)));
    Ok(ports)
}

/// Reads the counters of one port.
///
/// For each counter, the preferred file is `counters/<base>_64`, then
/// `<base>_64`, then `counters/<base>`, then `<base>`.
///
/// The `_64` variants are the 64-bit "extended" PMA counters, exposed by some
/// drivers (the NVIDIA/Mellanox OFED and DOCA stacks, for instance). They are
/// preferred because the legacy 32-bit PMA counters wrap around after 2^32
/// units, whereas the extended ones do not in practice. Both variants use the
/// same unit, so the conversion in [`crate::rate`] applies to both.
///
/// # Errors
///
/// Returns an error if a counter file exists but cannot be read, and if none of
/// the candidate files exists for a counter.
pub fn read_counters(root: &Path, key: &PortKey) -> anyhow::Result<RawCounters> {
    let port_dir = root.join(&key.device).join("ports").join(key.port.to_string());
    Ok(RawCounters {
        rcv_packets: read_counter(&port_dir, "port_rcv_packets")?,
        xmit_packets: read_counter(&port_dir, "port_xmit_packets")?,
        rcv_data: read_counter(&port_dir, "port_rcv_data")?,
        xmit_data: read_counter(&port_dir, "port_xmit_data")?,
    })
}

fn read_counter(port_dir: &Path, base: &str) -> anyhow::Result<u64> {
    let candidates = [
        port_dir.join("counters").join(format!("{base}_64")),
        port_dir.join(format!("{base}_64")),
        port_dir.join("counters").join(base),
        port_dir.join(base),
    ];

    for path in candidates {
        // Open and read in one step: a separate `is_file()` check would race
        // with the read, mask permission errors, and allow unbounded reads.
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("failed to open counter {}", path.display()));
            }
        };
        let mut content = Vec::new();
        file.take(64)
            .read_to_end(&mut content)
            .with_context(|| format!("failed to read counter {}", path.display()))?;
        let text = std::str::from_utf8(&content)
            .with_context(|| format!("invalid UTF-8 counter value in {}", path.display()))?;
        let value = text
            .trim()
            .parse::<u64>()
            .with_context(|| format!("invalid u64 counter value in {}", path.display()))?;
        return Ok(value);
    }

    Err(anyhow!(
        "no counter file found for {base} in {} (looked for {base}_64 and {base} in counters/ and the port directory)",
        port_dir.display()
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn create_port(root: &Path, device: &str, port: u32) -> std::path::PathBuf {
        let dir = root.join(device).join("ports").join(port.to_string()).join("counters");
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_counter(dir: &Path, name: &str, value: u64) {
        fs::write(dir.join(name), format!("{value}\n")).unwrap();
    }

    #[test]
    fn discovers_and_sorts_ports() {
        let root = tempdir().unwrap();
        create_port(root.path(), "mlx5_1", 1);
        create_port(root.path(), "mlx5_0", 2);
        create_port(root.path(), "mlx5_0", 1);
        // Non-numeric entries are ignored.
        fs::create_dir_all(root.path().join("mlx5_0/ports/not_a_port")).unwrap();

        let ports = discover_ports(root.path(), &Filters::default()).unwrap();
        assert_eq!(
            ports,
            vec![
                PortKey {
                    device: "mlx5_0".into(),
                    port: 1
                },
                PortKey {
                    device: "mlx5_0".into(),
                    port: 2
                },
                PortKey {
                    device: "mlx5_1".into(),
                    port: 1
                },
            ]
        );
    }

    #[test]
    fn discovers_symlinked_device_entries() {
        let root = tempdir().unwrap();
        let real = tempdir().unwrap();
        create_port(real.path(), "mlx5_0", 1);
        std::os::unix::fs::symlink(real.path().join("mlx5_0"), root.path().join("mlx5_0")).unwrap();

        let ports = discover_ports(root.path(), &Filters::default()).unwrap();
        assert_eq!(
            ports,
            vec![PortKey {
                device: "mlx5_0".into(),
                port: 1
            }]
        );
    }

    #[test]
    fn skips_unreadable_devices_and_keeps_healthy_ones() {
        let root = tempdir().unwrap();
        create_port(root.path(), "mlx5_0", 1);
        // A device directory without a `ports` subdirectory.
        fs::create_dir_all(root.path().join("broken")).unwrap();
        // A dangling symlink, like a device being hot-unplugged.
        std::os::unix::fs::symlink(root.path().join("missing"), root.path().join("dangling")).unwrap();

        let ports = discover_ports(root.path(), &Filters::default()).unwrap();
        assert_eq!(
            ports,
            vec![PortKey {
                device: "mlx5_0".into(),
                port: 1
            }]
        );
    }

    #[test]
    fn filters_devices_and_ports() {
        let root = tempdir().unwrap();
        create_port(root.path(), "mlx5_0", 1);
        create_port(root.path(), "mlx5_0", 2);
        create_port(root.path(), "mlx5_1", 1);

        let filters = Filters {
            devices: vec!["mlx5_0".into()],
            ports: vec![2],
        };
        let ports = discover_ports(root.path(), &filters).unwrap();
        assert_eq!(
            ports,
            vec![PortKey {
                device: "mlx5_0".into(),
                port: 2
            }]
        );
    }

    #[test]
    fn prefers_the_64_bit_counter_in_counters_directory() {
        let root = tempdir().unwrap();
        let counters = create_port(root.path(), "mlx5_0", 1);
        write_counter(&counters, "port_rcv_packets", 1);
        write_counter(&counters, "port_rcv_packets_64", 10);
        write_counter(&counters, "port_xmit_packets", 2);
        write_counter(&counters, "port_xmit_packets_64", 20);
        write_counter(&counters, "port_rcv_data", 3);
        write_counter(&counters, "port_rcv_data_64", 30);
        write_counter(&counters, "port_xmit_data", 4);
        write_counter(&counters, "port_xmit_data_64", 40);

        let key = PortKey {
            device: "mlx5_0".into(),
            port: 1,
        };
        let counters = read_counters(root.path(), &key).unwrap();
        assert_eq!(
            counters,
            RawCounters {
                rcv_packets: 10,
                xmit_packets: 20,
                rcv_data: 30,
                xmit_data: 40,
            }
        );
    }

    #[test]
    fn falls_back_to_plain_counter_when_64_bit_is_missing() {
        let root = tempdir().unwrap();
        let counters = create_port(root.path(), "mlx5_0", 1);
        write_counter(&counters, "port_rcv_packets", 11);
        write_counter(&counters, "port_xmit_packets", 12);
        write_counter(&counters, "port_rcv_data", 13);
        write_counter(&counters, "port_xmit_data", 14);

        let key = PortKey {
            device: "mlx5_0".into(),
            port: 1,
        };
        let counters = read_counters(root.path(), &key).unwrap();
        assert_eq!(
            counters,
            RawCounters {
                rcv_packets: 11,
                xmit_packets: 12,
                rcv_data: 13,
                xmit_data: 14,
            }
        );
    }
}
