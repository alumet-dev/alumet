use alumet::plugin::util::CounterDiff;
use nix::unistd::pipe;
use std::{
    fs::{File, create_dir_all},
    io,
    io::Write,
    os::fd::{
        OwnedFd, {FromRawFd, IntoRawFd},
    },
    path::Path,
};
use tempfile::{TempDir, tempdir};

use crate::perf_event::OpenedPowerEvent;
use crate::{domains::RaplDomainType, perf_event::PERF_MAX_ENERGY};

pub const SCALE: f32 = 2.3283064365386962890625e-10;

/// Entry to be created in the mock filesystem
pub enum EntryType<'a> {
    File(&'a str), // File with content
    Dir,           // Directory
}

/// Single entry specification
pub struct Entry<'a> {
    pub path: &'a str,
    pub entry_type: EntryType<'a>,
}

/// Create all specified entries under the given base path
#[cfg(test)]
pub fn create_mock_layout(base_path: &Path, entries: &[Entry]) -> io::Result<()> {
    for entry in entries {
        let full_path = base_path.join(entry.path);
        match &entry.entry_type {
            EntryType::Dir => create_dir_all(&full_path)?,
            EntryType::File(content) => {
                if let Some(parent) = full_path.parent() {
                    create_dir_all(parent)?;
                }
                let mut file = File::create(full_path)?;
                file.write_all(content.as_bytes())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn fake_opened_power_event() -> (OpenedPowerEvent, OwnedFd) {
    let (read_fd, write_fd) = pipe().unwrap();
    let file = unsafe { File::from_raw_fd(read_fd.into_raw_fd()) };
    (
        OpenedPowerEvent {
            fd: file,
            scale: SCALE.into(),
            domain: RaplDomainType::Package, // dummy value
            resource: RaplDomainType::Package.to_resource(0),
            counter: CounterDiff::with_max_value(PERF_MAX_ENERGY),
        },
        write_fd,
    )
}

#[cfg(test)]
pub fn create_valid_powercap_mock() -> anyhow::Result<TempDir> {
    use EntryType::{Dir, File};

    let base_path = tempdir()?;

    let entries = [
        Entry {
            path: "enabled",
            entry_type: File("1"),
        },
        Entry {
            path: "intel-rapl:0",
            entry_type: Dir,
        },
        Entry {
            path: "intel-rapl:0/name",
            entry_type: File("package-0"),
        },
        Entry {
            path: "intel-rapl:0/max_energy_range_uj",
            entry_type: File("262143328850"),
        },
        Entry {
            path: "intel-rapl:0/energy_uj",
            entry_type: File("124599532281"),
        },
        Entry {
            path: "intel-rapl:0/intel-rapl:0:0",
            entry_type: Dir,
        },
        Entry {
            path: "intel-rapl:0/intel-rapl:0:0/name",
            entry_type: File("core"),
        },
        Entry {
            path: "intel-rapl:0/intel-rapl:0:0/max_energy_range_uj",
            entry_type: File("262143328850"),
        },
        Entry {
            path: "intel-rapl:0/intel-rapl:0:0/energy_uj",
            entry_type: File("23893449269"),
        },
        Entry {
            path: "intel-rapl:0/intel-rapl:0:1",
            entry_type: Dir,
        },
        Entry {
            path: "intel-rapl:0/intel-rapl:0:1/name",
            entry_type: File("uncore"),
        },
        Entry {
            path: "intel-rapl:0/intel-rapl:0:1/max_energy_range_uj",
            entry_type: File("262143328850"),
        },
        Entry {
            path: "intel-rapl:0/intel-rapl:0:1/energy_uj",
            entry_type: File("23992349269"),
        },
        Entry {
            path: "intel-rapl:1",
            entry_type: Dir,
        },
        Entry {
            path: "intel-rapl:1/name",
            entry_type: File("psys"),
        },
        Entry {
            path: "intel-rapl:1/max_energy_range_uj",
            entry_type: File("262143328850"),
        },
        Entry {
            path: "intel-rapl:1/energy_uj",
            entry_type: File("154571208422"),
        },
        Entry {
            path: "intel-rapl:2",
            entry_type: Dir,
        },
        Entry {
            path: "intel-rapl:2/name",
            entry_type: File("dram"),
        },
        Entry {
            path: "intel-rapl:2/max_energy_range_uj",
            entry_type: File("262143328850"),
        },
        Entry {
            path: "intel-rapl:2/energy_uj",
            entry_type: File("182178908522"),
        },
    ];

    create_mock_layout(base_path.path(), &entries)?;
    Ok(base_path)
}
