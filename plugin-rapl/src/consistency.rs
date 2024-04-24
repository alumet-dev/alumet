use crate::{
    cpus::{self, CpuVendor},
    domains::RaplDomainType,
    perf_event::PowerEvent,
    powercap::{PowerZone, PowerZoneHierarchy},
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
    pub fn from_powercap_only(power_zones: PowerZoneHierarchy) -> Self {
        let power_zones = power_zones.flat;
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
pub fn check_domains_consistency(perf_events: Vec<PowerEvent>, power_zones: PowerZoneHierarchy) -> SafeSubset {
    // get all the domains available via perf-events
    let mut perf_rapl_domains: Vec<RaplDomainType> = perf_events.iter().map(|e| e.domain).collect();
    perf_rapl_domains.sort_by_key(|k| k.to_string());
    perf_rapl_domains.dedup_by_key(|k| k.to_string());

    // get all the domains available via Powercap
    let mut powercap_rapl_domains: Vec<RaplDomainType> = power_zones.flat.iter().map(|z| z.domain).collect();
    powercap_rapl_domains.sort_by_key(|k| k.to_string());
    powercap_rapl_domains.dedup_by_key(|k| k.to_string());

    // warn for inconsistencies
    if perf_rapl_domains != powercap_rapl_domains {
        log::warn!("Powercap and perf-event don't report the same RAPL domains. This may be due to a bug in powercap or in perf-event.");
        log::warn!("Upgrading to a newer kernel could fix the problem.");
        log::warn!("Perf-event: {}", mkstring(&perf_rapl_domains, ", "));
        log::warn!("Powercap:   {}", mkstring(&powercap_rapl_domains, ", "));

        match cpus::cpu_vendor() {
            Ok(CpuVendor::Amd) =>
                log::warn!(
                    "AMD cpus only supports the \"pkg\" domain (and sometimes \"core\"), but their support is buggy on old Linux kernels!

                    - All events are present in the sysfs, but they should not be there. This seems to have been fixed in Linux 5.17.
                    See https://github.com/torvalds/linux/commit/0036fb00a756a2f6e360d44e2e3d2200a8afbc9b.

                    - The \"core\" domain doesn't work in perf-event, it could be added soon, if it's supported.
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
            .into_iter()
            .filter(|e| domains_subset.contains(&e.domain))
            .collect();
        let power_zones_subset = power_zones
            .flat
            .into_iter()
            .filter(|z| domains_subset.contains(&z.domain))
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
            perf_events,
            power_zones: power_zones.flat,
            is_whole: true,
        }
    }
}

/// Takes a slice of elements that can be converted to strings, converts them and joins them all.
pub(crate) fn mkstring<A: ToString>(elems: &[A], sep: &str) -> String {
    elems.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(sep)
}
