// See https://www.kernel.org/doc/html/latest/power/powercap/powercap.html
// for an explanation of the Power Capping framework.

use std::{
    fmt::Display,
    fs::{self, File},
    io::{Read, Seek},
    path::{Path, PathBuf},
};

use alumet::metrics::TypedMetricId;
use alumet::plugin::util::{CounterDiff, CounterDiffUpdate};
use alumet::resources::Resource;
use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    resources::ResourceConsumer,
};
use anyhow::{anyhow, Context};

use super::domains::RaplDomainType;

const POWERCAP_RAPL_PATH: &str = "/sys/devices/virtual/powercap/intel-rapl";
const POWER_ZONE_PREFIX: &str = "intel-rapl";
const POWERCAP_ENERGY_UNIT: f64 = 0.000_001; // 1 microJoules

const PERMISSION_ADVICE: &str = "Try to adjust file permissions.";

/// Hierarchy of power zones
pub struct PowerZoneHierarchy {
    /// All the zones in the same Vec.
    pub flat: Vec<PowerZone>,
    /// The top zones. To access their children, use [PowerZone::children].
    pub top: Vec<PowerZone>,
}

/// A power zone.
#[derive(Debug, Clone)]
pub struct PowerZone {
    /// The name of the zone, as returned by powercap, for instance `package-0` or `core`.
    pub name: String,

    /// The RAPL domain type, as an enum
    pub domain: RaplDomainType,

    /// The path of the zone in sysfs, for instance
    /// `/sys/devices/virtual/powercap/intel-rapl/intel-rapl:0`.
    ///
    /// Note that in the above path, `intel-rapl` is the "control type"
    /// and "intel-rapl:0" is the power zone.
    /// On my machine, that zone is named `package-0`.
    pub path: PathBuf,

    /// The sub-zones (can be empty).
    pub children: Vec<PowerZone>,

    /// The id of the socket that "contains" this zone, if applicable (psys has no socket)
    pub socket_id: Option<u32>,
}

impl PowerZone {
    pub fn energy_path(&self) -> PathBuf {
        self.path.join("energy_uj")
    }

    pub fn max_energy_path(&self) -> PathBuf {
        self.path.join("max_energy_range_uj")
    }

    fn fmt_rec(&self, f: &mut std::fmt::Formatter<'_>, level: i8) -> std::fmt::Result {
        let mut indent = "  ".repeat(level as _);
        if level > 0 {
            indent.insert(0, '\n');
        }

        let powercap_name = &self.name;
        let domain = self.domain;
        let path = self.path.to_string_lossy();

        write!(f, "{indent}- {powercap_name} ({domain:?}) \t\t: {path}")?;
        for subzone in &self.children {
            subzone.fmt_rec(f, level + 1)?;
        }
        Ok(())
    }
}

impl Display for PowerZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_rec(f, 0)
    }
}

/// Discovers all the RAPL power zones in the powercap sysfs.
pub fn all_power_zones() -> anyhow::Result<PowerZoneHierarchy> {
    fn parse_zone_name(name: &str) -> Option<RaplDomainType> {
        match name {
            "psys" => Some(RaplDomainType::Platform),
            "core" => Some(RaplDomainType::PP0),
            "uncore" => Some(RaplDomainType::PP1),
            "dram" => Some(RaplDomainType::Dram),
            _ if name.starts_with("package-") => Some(RaplDomainType::Package),
            _ => None,
        }
    }

    /// Recursively explore a power zone
    fn explore_rec(
        dir: &Path,
        parent_socket: Option<u32>,
        flat: &mut Vec<PowerZone>,
    ) -> anyhow::Result<Vec<PowerZone>> {
        let mut zones = Vec::new();
        for e in fs::read_dir(dir)? {
            let entry = e?;
            let path = entry.path();
            let file_name = path.file_name().unwrap().to_string_lossy();

            if path.is_dir() && file_name.starts_with(POWER_ZONE_PREFIX) {
                let name_path = path.join("name");
                let name = fs::read_to_string(&name_path)?.trim().to_owned();
                let socket_id = {
                    if let Some(parent_id) = parent_socket {
                        Some(parent_id)
                    } else if let Some(id_str) = name.strip_prefix("package-") {
                        let id: u32 = id_str
                            .parse()
                            .with_context(|| format!("Failed to extract package id from '{name}'"))?;
                        Some(id)
                    } else {
                        None
                    }
                };
                let domain = parse_zone_name(&name).with_context(|| format!("Unknown RAPL powercap zone {name}"))?;
                let children = explore_rec(&path, socket_id, flat)?; // recursively explore
                let zone = PowerZone {
                    name,
                    domain,
                    path,
                    children,
                    socket_id,
                };
                zones.push(zone.clone());
                flat.push(zone);
            }
        }
        zones.sort_by_key(|z| z.path.to_string_lossy().to_string());
        Ok(zones)
    }
    let mut flat = Vec::new();
    let top = explore_rec(Path::new(POWERCAP_RAPL_PATH), None, &mut flat)
        .with_context(|| format!("Could not explore {POWERCAP_RAPL_PATH}. {PERMISSION_ADVICE}"))?;
    Ok(PowerZoneHierarchy { flat, top })
}

/// Powercap probe
pub struct PowercapProbe {
    metric: TypedMetricId<f64>,

    /// Ready-to-use powercap zones with additional metadata
    zones: Vec<OpenedZone>,
}

struct OpenedZone {
    file: File,
    domain: RaplDomainType,
    /// The corresponding ResourceId
    resource: Resource,
    /// Overflow-correcting counter, to compute the energy consumption difference.
    counter: CounterDiff,
}

impl PowercapProbe {
    pub fn new(metric: TypedMetricId<f64>, zones: &[PowerZone]) -> anyhow::Result<PowercapProbe> {
        if zones.is_empty() {
            return Err(anyhow!("At least one power zone is required for PowercapProbe"))?;
        }

        let mut opened = Vec::with_capacity(zones.len());
        for zone in zones {
            let file = File::open(zone.energy_path()).with_context(|| {
                format!(
                    "Could not open {}. {PERMISSION_ADVICE}",
                    zone.energy_path().to_string_lossy()
                )
            })?;

            let str_max_energy_uj = fs::read_to_string(zone.max_energy_path()).with_context(|| {
                format!(
                    "Could not read {}. {PERMISSION_ADVICE}",
                    zone.max_energy_path().to_string_lossy()
                )
            })?;

            let max_energy_uj = str_max_energy_uj
                .trim_end()
                .parse()
                .with_context(|| format!("parse max_energy_uj: '{str_max_energy_uj}'"))?;

            let socket = zone.socket_id.unwrap_or(0); // put psys in socket 0

            let counter = CounterDiff::with_max_value(max_energy_uj);
            let opened_zone = OpenedZone {
                file,
                domain: zone.domain,
                resource: zone.domain.to_resource(socket),
                counter,
            };
            opened.push(opened_zone);
        }

        Ok(PowercapProbe { metric, zones: opened })
    }
}

impl alumet::pipeline::Source for PowercapProbe {
    fn poll(
        &mut self,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) -> Result<(), alumet::pipeline::PollError> {
        // reuse the same buffer for all the zones
        // the size of the content of the file `energy_uj` should never exceed those of `max_energy_uj`,
        // which is 16 bytes on all our test machines
        let mut zone_reading_buf = Vec::with_capacity(16);

        for zone in &mut self.zones {
            // read the file from the beginning
            zone.file.rewind()?;
            zone.file.read_to_end(&mut zone_reading_buf)?;

            // parse the content of the file
            let content = std::str::from_utf8(&zone_reading_buf)?;
            let counter_value: u64 = content
                .trim_end()
                .parse()
                .with_context(|| format!("failed to parse {:?}: '{content}'", zone.file))?;

            // store the value, handle the overflow if there is one
            let diff = match zone.counter.update(counter_value) {
                CounterDiffUpdate::FirstTime => None,
                CounterDiffUpdate::Difference(diff) => Some(diff),
                CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
            };
            if let Some(value) = diff {
                let joules = (value as f64) * POWERCAP_ENERGY_UNIT;
                let consumer = ResourceConsumer::LocalMachine;
                measurements.push(
                    MeasurementPoint::new(timestamp, self.metric, zone.resource.clone(), consumer, joules)
                        .with_attr("domain", AttributeValue::String(zone.domain.to_string())),
                )
            };

            // clear the buffer, so that we can fill it again
            zone_reading_buf.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::all_power_zones;

    #[test]
    fn test_powercap() {
        let zones = all_power_zones().expect("failed to get powercap power zones");
        println!("---- Hierarchy ----");
        for z in zones.top {
            println!("{z}");
        }
        println!("---- Flat list ----");
        for z in zones.flat {
            println!("{z}")
        }
    }
}
