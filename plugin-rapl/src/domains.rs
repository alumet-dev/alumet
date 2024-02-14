use std::{fmt, str::FromStr};

use alumet::metrics::ResourceId;

use crate::{perf_event::PowerEvent, powercap::PowerZoneHierarchy};

// use enum_map::{self, EnumMap};

/// A known RAPL domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaplDomainType {
    /// entire socket
    Package,
    /// power plane 0: core
    PP0,
    /// power plane 1: uncore
    PP1,
    ///  DRAM
    Dram,
    /// psys (only available on recent client platforms like laptops)
    Platform,
}

impl fmt::Display for RaplDomainType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl FromStr for RaplDomainType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "package" | "pkg" => Ok(RaplDomainType::Package),
            "pp0" | "core" => Ok(RaplDomainType::PP0),
            "pp1" | "uncore" => Ok(RaplDomainType::PP1),
            "dram" | "ram" => Ok(RaplDomainType::Dram),
            "platform" | "psys" => Ok(RaplDomainType::Platform),
            _ => Err(s.to_owned()),
        }
    }
}

impl RaplDomainType {
    pub const ALL: [RaplDomainType; 5] = [
        RaplDomainType::Package,
        RaplDomainType::PP0,
        RaplDomainType::PP1,
        RaplDomainType::Dram,
        RaplDomainType::Platform,
    ];

    pub const ALL_IN_ADDR_ORDER: [RaplDomainType; 5] = [
        RaplDomainType::Package,
        RaplDomainType::Dram,
        RaplDomainType::PP0,
        RaplDomainType::PP1,
        RaplDomainType::Platform,
    ];

    pub fn to_resource(&self, pkg_id: u32) -> ResourceId {
        match self {
            RaplDomainType::Package => ResourceId::CpuPackage { id: pkg_id },
            // PP0 records data for all the cores of a package, not individual cores
            RaplDomainType::PP0 => ResourceId::CpuPackage { id: pkg_id },
            RaplDomainType::PP1 => ResourceId::CpuPackage { id: pkg_id },
            RaplDomainType::Dram => ResourceId::Dram { pkg_id },
            RaplDomainType::Platform => ResourceId::LocalMachine,
        }
    }
}
