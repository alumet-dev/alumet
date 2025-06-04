//! Parse /proc/mounts.

use thiserror::Error;

pub const PROC_MOUNTS_PATH: &str = "/proc/mounts";

/// A mounted filesystem.
#[derive(Debug, PartialEq, Eq)]
pub struct Mount {
    pub spec: String,
    pub mount_point: String,
    pub fs_type: String,
    pub mount_options: Vec<String>,
    // we don't need the other fields
}

#[derive(Debug, Error)]
#[error("invalid mount line: {input}")]
pub struct ParseError {
    pub(crate) input: String,
}

#[derive(Debug, Error)]
pub enum ReadError {
    #[error("failed to parse {PROC_MOUNTS_PATH}")]
    Parse(#[from] ParseError),
    #[error("failed to read {PROC_MOUNTS_PATH}")]
    Io(#[from] std::io::Error),
}

impl Mount {
    /// Attempts to parse a line of `/proc/mounts`.
    /// Returns `None` if it fails.
    pub(crate) fn parse(line: &str) -> Option<Self> {
        let mut fields = line.split_ascii_whitespace().into_iter();
        let spec = fields.next()?.to_string();
        let mount_point = fields.next()?.to_string();
        let fs_type = fields.next()?.to_string();
        let mount_options = fields.next()?.split(',').map(ToOwned::to_owned).collect();
        Some(Self {
            spec,
            mount_point,
            fs_type,
            mount_options,
        })
    }
}

pub(crate) fn read_proc_mounts() -> Result<Vec<Mount>, ReadError> {
    let mut buf = Vec::with_capacity(8);
    let content = std::fs::read_to_string(PROC_MOUNTS_PATH).map_err(ReadError::from)?;
    parse_proc_mounts(&content, &mut buf).map_err(ReadError::from)?;
    Ok(buf)
}

pub(crate) fn parse_proc_mounts(content: &str, buf: &mut Vec<Mount>) -> Result<(), ParseError> {
    for line in content.lines() {
        let line = line.trim_ascii_start();
        if !line.is_empty() && !line.starts_with('#') {
            let m = Mount::parse(line).ok_or_else(|| ParseError { input: line.to_owned() })?;
            buf.push(m);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{parse_proc_mounts, Mount};

    fn vec_str(values: &[&str]) -> Vec<String> {
        values.into_iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parsing() {
        let content = "
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
tmpfs /run tmpfs rw,nosuid,nodev,noexec,relatime,size=1599352k,mode=755,inode64 0 0
cgroup2 /sys/fs/cgroup cgroup2 rw,nosuid,nodev,noexec,relatime,nsdelegate,memory_recursiveprot 0 0
/dev/nvme0n1p1 /boot/efi vfat rw,relatime,errors=remount-ro 0 0";
        let mut mounts = Vec::new();
        parse_proc_mounts(&content, &mut mounts).unwrap();

        let expected = vec![
            Mount {
                spec: String::from("sysfs"),
                mount_point: String::from("/sys"),
                fs_type: String::from("sysfs"),
                mount_options: vec_str(&["rw", "nosuid", "nodev", "noexec", "relatime"]),
            },
            Mount {
                spec: String::from("tmpfs"),
                mount_point: String::from("/run"),
                fs_type: String::from("tmpfs"),
                mount_options: vec_str(&[
                    "rw",
                    "nosuid",
                    "nodev",
                    "noexec",
                    "relatime",
                    "size=1599352k",
                    "mode=755",
                    "inode64",
                ]),
            },
            Mount {
                spec: String::from("cgroup2"),
                mount_point: String::from("/sys/fs/cgroup"),
                fs_type: String::from("cgroup2"),
                mount_options: vec_str(&[
                    "rw",
                    "nosuid",
                    "nodev",
                    "noexec",
                    "relatime",
                    "nsdelegate",
                    "memory_recursiveprot",
                ]),
            },
            Mount {
                spec: String::from("/dev/nvme0n1p1"),
                mount_point: String::from("/boot/efi"),
                fs_type: String::from("vfat"),
                mount_options: vec_str(&["rw", "relatime", "errors=remount-ro"]),
            },
        ];
        assert_eq!(expected, mounts);
    }

    #[test]
    fn parsing_error() {
        let mut mounts = Vec::new();
        parse_proc_mounts("badbad", &mut mounts).unwrap_err();
        parse_proc_mounts("croup2 /sys/fs/cgroup", &mut mounts).unwrap_err();
    }

    #[test]
    fn parsing_comments() {
        let mut mounts = Vec::new();
        parse_proc_mounts("\n# badbad\n", &mut mounts).unwrap();
    }
}
