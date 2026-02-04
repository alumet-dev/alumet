use crate::{
    cpus::{self, CpuVendor},
    domains::RaplDomainType,
    perf_event::PowerEvent,
    powercap::PowerZone,
};
use anyhow::anyhow;

#[derive(Debug)]
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
        log::warn!(
            "Powercap and perf_events don't report the same RAPL domains. This may be caused by a bug in powercap or in perf_events."
        );
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

pub fn get_available_domains(
    power_events: anyhow::Result<Vec<PowerEvent>>,
    power_zones: anyhow::Result<Vec<PowerZone>>,
    check_consistency: bool,
    use_perf: &mut bool,
    use_powercap: &mut bool,
) -> anyhow::Result<(SafeSubset, String)> {
    Ok(match (power_events, power_zones) {
        (Ok(power_events), Ok(power_zones)) => {
            if !check_consistency {
                // here it assumes that powercap is alive
                (
                    SafeSubset::from_powercap_only(power_zones),
                    " (from powercap)".to_string(),
                )
            } else {
                let mut safe_domains = check_domains_consistency(&power_events, &power_zones);
                let mut domain_origin = String::from("");
                if !safe_domains.is_whole {
                    // If one of the domain set is smaller, it could be empty, which would prevent the plugin from measuring anything.
                    // In that case, we fall back to the other interface, the one that reports a non-empty list of domains.
                    if power_events.is_empty() && !power_zones.is_empty() {
                        log::warn!(
                            "perf_events returned an empty list of RAPL domains, I will disable perf_events and use powercap instead."
                        );
                        *use_perf = false;
                        safe_domains = SafeSubset::from_powercap_only(power_zones);
                        domain_origin = " (from powercap)".to_string();
                    } else if !power_events.is_empty() && power_zones.is_empty() {
                        log::warn!(
                            "powercap returned an empty list of RAPL domains, I will disable powercap and use perf_events instead."
                        );
                        *use_powercap = false;
                        safe_domains = SafeSubset::from_perf_only(power_events);
                        domain_origin = " (from perf_events)".to_string();
                    } else {
                        domain_origin = " (\"safe subset\")".to_string();
                    }
                }
                (safe_domains, domain_origin)
            }
        }
        (Ok(power_events), Err(powercap_err)) => {
            log::error!("Cannot read the list of RAPL domains available via the powercap interface: {powercap_err:?}.");
            log::warn!(
                "The consistency of the RAPL domains reported by the different interfaces of the Linux kernel cannot be checked (this is useful to work around bugs in some kernel versions on some machines)."
            );
            (
                SafeSubset::from_perf_only(power_events),
                " (from perf_events)".to_string(),
            )
        }
        (Err(perf_err), Ok(power_zones)) => {
            log::warn!("Cannot read the list of RAPL domains available via the perf_events interface: {perf_err:?}.");
            log::warn!(
                "The consistency of the RAPL domains reported by the different interfaces of the Linux kernel cannot be checked (this is useful to work around bugs in some kernel versions on some machines)."
            );
            if *use_perf {
                log::warn!("Because of the previous error, I will disable perf_events and fall back to powercap.");
            }
            *use_perf = false;
            (
                SafeSubset::from_powercap_only(power_zones),
                " (from powercap)".to_string(),
            )
        }
        (Err(perf_err), Err(power_err)) => {
            log::error!(
                "I could use neither perf_events nor powercap.\nperf_events error: {perf_err:?}\npowercap error: {power_err:?}"
            );
            Err(anyhow!(
                "Both perf_events and powercap failed, unable to read RAPL counters: {perf_err}\n{power_err}"
            ))?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        consistency::get_available_domains, domains::RaplDomainType, perf_event::PowerEvent, powercap::PowerZone,
    };
    use std::path::Path;

    const SCALE: f32 = 2.3283064365386962890625e-10;

    fn perf_event(domain: RaplDomainType) -> PowerEvent {
        PowerEvent {
            name: domain.to_string(),
            domain,
            code: 0,
            unit: "J".into(),
            scale: SCALE,
        }
    }

    fn power_zone(domain: RaplDomainType) -> PowerZone {
        PowerZone {
            name: domain.to_string(),
            domain,
            path: std::path::Path::new("/tmp").to_path_buf(),
            socket_id: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn test_same_domain() -> anyhow::Result<()> {
        let power_events = vec![PowerEvent {
            name: "pkg".to_string(),
            domain: RaplDomainType::Package,
            code: 2,
            unit: "Joules".to_string(),
            scale: SCALE,
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
            scale: SCALE,
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

    #[test]
    fn test_mkstring_with_rapl_domain_type() {
        let elems = vec![RaplDomainType::Package, RaplDomainType::PP0];
        let result = mkstring(&elems, ", ");
        assert_eq!(result, "package, pp0");
    }

    #[test]
    fn test_get_available_domains_when_perf_empty() {
        let power_events = Ok(vec![]);
        let power_zones = Ok(vec![power_zone(RaplDomainType::Package)]);

        let mut use_perf = true;
        let mut use_powercap = true;
        let check_consistency = true;

        let (subset, origin) = get_available_domains(
            power_events,
            power_zones,
            check_consistency,
            &mut use_perf,
            &mut use_powercap,
        )
        .unwrap();

        assert_eq!(origin, " (from powercap)");
        assert_eq!(subset.domains, vec![RaplDomainType::Package]);
    }

    #[test]
    fn test_get_available_domains_when_powercap_empty() {
        let power_events = Ok(vec![perf_event(RaplDomainType::Package)]);
        let power_zones = Ok(vec![]);

        let mut use_perf = true;
        let mut use_powercap = true;
        let check_consistency = true;

        let (subset, origin) = get_available_domains(
            power_events,
            power_zones,
            check_consistency,
            &mut use_perf,
            &mut use_powercap,
        )
        .unwrap();

        assert_eq!(origin, " (from perf_events)");
        assert_eq!(subset.domains, vec![RaplDomainType::Package]);
    }

    #[test]
    fn test_get_available_domains_safe_subset_when_domains_differ() {
        let power_events = Ok(vec![
            perf_event(RaplDomainType::Package),
            perf_event(RaplDomainType::PP0),
        ]);

        let power_zones = Ok(vec![power_zone(RaplDomainType::Package)]);

        let mut use_perf = true;
        let mut use_powercap = true;
        let check_consistency = true;

        let (subset, origin) = get_available_domains(
            power_events,
            power_zones,
            check_consistency,
            &mut use_perf,
            &mut use_powercap,
        )
        .unwrap();

        assert_eq!(origin, " (\"safe subset\")");
        assert_eq!(subset.domains, vec![RaplDomainType::Package]);
    }

    #[test]
    fn test_get_available_domains_when_powercap_errors() {
        let power_events = Ok(vec![PowerEvent {
            name: "pkg".to_string(),
            domain: RaplDomainType::Package,
            code: 0,
            unit: "J".into(),
            scale: SCALE,
        }]);

        let power_zones = Err(anyhow!("powercap broken"));

        let mut use_perf = true;
        let mut use_powercap = true;
        let check_consistency = true;

        let (subset, origin) = get_available_domains(
            power_events,
            power_zones,
            check_consistency,
            &mut use_perf,
            &mut use_powercap,
        )
        .unwrap();

        assert_eq!(origin, " (from perf_events)");
        assert_eq!(subset.domains, vec![RaplDomainType::Package]);
        assert!(subset.power_zones.is_empty());
    }

    #[test]
    fn test_get_available_domains_when_both_perf_and_powercap_error() {
        let power_events = Err(anyhow!("perf_events broken"));
        let power_zones = Err(anyhow!("powercap broken"));

        let mut use_perf = true;
        let mut use_powercap = true;
        let check_consistency = true;

        let result = get_available_domains(
            power_events,
            power_zones,
            check_consistency,
            &mut use_perf,
            &mut use_powercap,
        )
        .unwrap_err();

        assert!(result.to_string().contains("Both perf_events and powercap failed"));
    }
}
