use crate::{
    cpus::{self, CpuVendor},
    domains::RaplDomainType,
    perf_event::PowerEvent,
    powercap::PowerZone,
};

pub struct SafeSubset {
    pub domains: Vec<RaplDomainType>,
    pub perf_events: Vec<PowerEvent>,
    pub power_zones: Vec<PowerZone>,
    pub is_whole: bool,
}

impl SafeSubset {
    pub fn from_perf_only(perf_events: Vec<PowerEvent>) -> Self {
        let mut domains: Vec<RaplDomainType> = perf_events.iter().map(|e| e.domain).collect();
        domains.sort_by_key(|k| k.to_string());
        domains.dedup_by_key(|k| k.to_string());
        Self {
            domains,
            perf_events,
            power_zones: Vec::new(),
            is_whole: true,
        }
    }

    #[allow(dead_code)]
    pub fn from_powercap_only(power_zones: Vec<PowerZone>) -> Self {
        let mut domains: Vec<RaplDomainType> = power_zones.iter().map(|z| z.domain).collect();
        domains.sort_by_key(|k| k.to_string());
        domains.dedup_by_key(|k| k.to_string());
        Self {
            domains,
            perf_events: Vec::new(),
            power_zones,
            is_whole: true,
        }
    }
}

/// Checks the consistency of the RAPL domains reported by the different interfaces of the Linux kernel,
/// and returns the list of domains that are available everywhere ("safe subset").
pub fn check_domains_consistency(perf_events: &Vec<PowerEvent>, power_zones: &Vec<PowerZone>) -> SafeSubset {
    // get all the domains available via perf_events
    let mut perf_rapl_domains: Vec<RaplDomainType> = perf_events.iter().map(|e| e.domain).collect();
    perf_rapl_domains.sort_by_key(|k| k.to_string());
    perf_rapl_domains.dedup_by_key(|k| k.to_string());

    // get all the domains available via Powercap
    let mut powercap_rapl_domains: Vec<RaplDomainType> = power_zones.iter().map(|z| z.domain).collect();
    powercap_rapl_domains.sort_by_key(|k| k.to_string());
    powercap_rapl_domains.dedup_by_key(|k| k.to_string());

    // warn for inconsistencies
    if perf_rapl_domains != powercap_rapl_domains {
        log::warn!("Powercap and perf_events don't report the same RAPL domains. This may be caused by a bug in powercap or in perf_events.");
        log::warn!("Upgrading to a newer kernel could fix the problem.");
        log::warn!("Perf_events: {}", mkstring(&perf_rapl_domains, ", "));
        log::warn!("Powercap:    {}", mkstring(&powercap_rapl_domains, ", "));

        match cpus::cpu_vendor() {
            Ok(CpuVendor::Amd) =>
                log::warn!(
                    "AMD cpus only supports the \"pkg\" domain (and sometimes \"core\"), but their support is buggy on old Linux kernels!

                    - All events are present in the sysfs, but they should not be there. This seems to have been fixed in Linux 5.17.
                    See https://github.com/torvalds/linux/commit/0036fb00a756a2f6e360d44e2e3d2200a8afbc9b.

                    - The \"core\" domain doesn't work in perf_events, it could be added soon, if it's supported.
                    See https://lore.kernel.org/lkml/20230217161354.129442-1-wyes.karny@amd.com/T/.

                    NOTE: It could also be totally unsupported, because it gives erroneous/aberrant values in powercap on our bi-socket AMD EPYC 7702 64-core Processor.
                    "
                ),
            Ok(_) => (),
            Err(e) =>
                // not dramatic, we can proceed
                log::warn!(
                    "Failed to detect the cpu vendor. {}",
                    e
                ),
        };

        // compute the "safe" subset
        let mut domains_subset = Vec::new();
        for d in perf_rapl_domains {
            if powercap_rapl_domains.contains(&d) {
                domains_subset.push(d);
            }
        }
        let perf_events_subset = perf_events
            .iter()
            .filter(|e| domains_subset.contains(&e.domain))
            .cloned()
            .collect();
        let power_zones_subset = power_zones
            .iter()
            .filter(|z| domains_subset.contains(&z.domain))
            .cloned()
            .collect();
        SafeSubset {
            domains: domains_subset,
            perf_events: perf_events_subset,
            power_zones: power_zones_subset,
            is_whole: false,
        }
    } else {
        SafeSubset {
            domains: perf_rapl_domains,
            perf_events: perf_events.to_owned(),
            power_zones: power_zones.to_owned(),
            is_whole: true,
        }
    }
}

/// Takes a slice of elements that can be converted to strings, converts them and joins them all.
pub(crate) fn mkstring<A: ToString>(elems: &[A], sep: &str) -> String {
    elems.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(sep)
}

#[cfg(test)]
mod tests {
    use super::check_domains_consistency;
    use crate::{domains::RaplDomainType, perf_event::PowerEvent, powercap::PowerZone};
    use std::path::Path;

    #[test]
    fn test_same_domain() -> anyhow::Result<()> {
        let power_events = vec![PowerEvent {
            name: "pkg".to_string(),
            domain: RaplDomainType::Package,
            code: 2,
            unit: "Joules".to_string(),
            scale: 2.3283064365386962890625e-10,
        }];

        let power_zones = vec![PowerZone {
            name: "package-0".to_string(),
            domain: RaplDomainType::Package,
            path: Path::new("/sys/devices/virtual/powercap/intel-rapl/intel-rapl:0").to_path_buf(),
            socket_id: Some(0),
            children: Vec::new(),
        }];

        let safe_subset = check_domains_consistency(&power_events, &power_zones);

        assert_eq!(safe_subset.is_whole, true);
        assert_eq!(safe_subset.domains, vec![RaplDomainType::Package]);
        assert_eq!(safe_subset.perf_events, power_events);
        assert_eq!(safe_subset.power_zones, power_zones);
        Ok(())
    }

    #[test]
    fn test_different_domain() -> anyhow::Result<()> {
        let power_events = vec![PowerEvent {
            name: "pkg".to_string(),
            domain: RaplDomainType::Package,
            code: 2,
            unit: "Joules".to_string(),
            scale: 2.3283064365386962890625e-10,
        }];

        let power_zones = vec![
            PowerZone {
                name: "package-0".to_string(),
                domain: RaplDomainType::Package,
                path: Path::new("/sys/devices/virtual/powercap/intel-rapl/intel-rapl:0").to_path_buf(),
                socket_id: Some(0),
                children: Vec::new(),
            },
            PowerZone {
                name: "core".to_string(),
                domain: RaplDomainType::PP0,
                path: Path::new("/sys/devices/virtual/powercap/intel-rapl/intel-rapl:0/intel-rapl:0:0").to_path_buf(),
                socket_id: Some(0),
                children: Vec::new(),
            },
        ];

        let safe_subset = check_domains_consistency(&power_events, &power_zones);

        assert_eq!(safe_subset.is_whole, false);
        assert_eq!(safe_subset.domains, vec![RaplDomainType::Package]);
        assert_eq!(safe_subset.perf_events, power_events);
        assert_eq!(
            safe_subset.power_zones,
            vec![PowerZone {
                name: "package-0".to_string(),
                domain: RaplDomainType::Package,
                path: Path::new("/sys/devices/virtual/powercap/intel-rapl/intel-rapl:0").to_path_buf(),
                socket_id: Some(0),
                children: Vec::new(),
            },]
        );
        Ok(())
    }
}
