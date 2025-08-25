use core::f64;

use enum_map::{EnumMap, enum_map};

use crate::domains::RaplDomainType;

/// Computes total values per RAPL domain type.
pub struct DomainTotals {
    per_domain: EnumMap<RaplDomainType, f64>,
}

impl DomainTotals {
    pub fn new() -> Self {
        Self {
            per_domain: enum_map! { _ => f64::NAN },
        }
    }

    pub fn push(&mut self, domain: RaplDomainType, value: f64) {
        if self.per_domain[domain].is_nan() {
            self.per_domain[domain] = value;
        } else {
            self.per_domain[domain] += value;
        }
    }

    pub fn iter(&mut self) -> impl Iterator<Item = (RaplDomainType, f64)> {
        self.per_domain
            .iter()
            .filter(|(_, v)| !v.is_nan())
            .map(|(k, v)| (k, *v))
    }

    pub fn reset(&mut self) {
        for v in self.per_domain.as_mut_slice() {
            *v = f64::NAN;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_totals() {
        let mut t = DomainTotals::new();
        assert!(t.iter().collect::<Vec<_>>().is_empty());

        t.push(RaplDomainType::Dram, 15.1);
        t.push(RaplDomainType::Dram, 0.9);
        t.push(RaplDomainType::Package, 2.0);
        t.push(RaplDomainType::PP0, 0.0);

        assert_eq!(t.per_domain[RaplDomainType::Dram], 16.0);
        assert_eq!(t.per_domain[RaplDomainType::Package], 2.0);
        assert_eq!(t.per_domain[RaplDomainType::PP0], 0.0);

        let mut present: Vec<RaplDomainType> = t.iter().map(|(k, _)| k).collect();
        present.sort();
        let mut expect_present = vec![RaplDomainType::Package, RaplDomainType::Dram, RaplDomainType::PP0];
        expect_present.sort();
        assert_eq!(present, expect_present);

        t.reset();
        assert!(t.iter().collect::<Vec<_>>().is_empty());
    }
}
