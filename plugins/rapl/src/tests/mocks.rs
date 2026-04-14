use std::fs::{self, File};
use std::{io::Write, path::Path};
use tempfile::{TempDir, tempdir};

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
pub fn create_mock_layout(base_path: &Path, entries: &[Entry]) -> std::io::Result<()> {
    for entry in entries {
        let full_path = base_path.join(entry.path);
        match &entry.entry_type {
            EntryType::Dir => fs::create_dir_all(&full_path)?,
            EntryType::File(content) => {
                if let Some(parent) = full_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut file = File::create(full_path)?;
                file.write_all(content.as_bytes())?;
            }
        }
    }
    Ok(())
}

pub fn create_valid_powercap_mock() -> anyhow::Result<TempDir> {
    let tmp = tempdir()?;

    use EntryType::*;

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

    create_mock_layout(tmp.path(), &entries)?;
    Ok(tmp)
}
