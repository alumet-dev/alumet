use std::ops::{AddAssign, Index};

use enum_map::{EnumMap, enum_map};

use crate::hwmon::SensorTagKind;

pub struct PerKindTotals<V> {
    per_kind: EnumMap<SensorTagKind, Option<V>>,
}

impl<V> PerKindTotals<V>
where
    V: AddAssign + Copy + PartialOrd,
{
    pub fn new() -> Self {
        Self {
            per_kind: enum_map! { _ => None },
        }
    }

    pub fn push(&mut self, kind: SensorTagKind, value: V) {
        match &mut self.per_kind[kind] {
            Some(total) => *total += value,
            total @ None => *total = Some(value),
        }
    }

    pub fn iter(&mut self) -> impl Iterator<Item = (SensorTagKind, V)> {
        self.per_kind.iter().filter_map(|(k, v)| match v {
            Some(v) => Some((k, *v)),
            None => None,
        })
    }
}

impl<V> Index<SensorTagKind> for PerKindTotals<V> {
    type Output = Option<V>;

    fn index(&self, index: SensorTagKind) -> &Self::Output {
        &self.per_kind[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_totals() {
        let mut t = PerKindTotals::new();
        assert!(t.iter().collect::<Vec<_>>().is_empty());

        t.push(SensorTagKind::Grace, 15.1);
        t.push(SensorTagKind::Grace, 0.9);
        t.push(SensorTagKind::Module, 2.0);
        t.push(SensorTagKind::SysIo, 0.0);

        assert_eq!(t[SensorTagKind::Cpu], None);
        assert_eq!(t[SensorTagKind::Grace], Some(16.0));
        assert_eq!(t[SensorTagKind::Module], Some(2.0));
        assert_eq!(t[SensorTagKind::SysIo], Some(0.0));

        let mut present: Vec<SensorTagKind> = t.iter().map(|(k, _)| k).collect();
        present.sort();
        let mut expect_present = vec![SensorTagKind::Grace, SensorTagKind::Module, SensorTagKind::SysIo];
        expect_present.sort();
        assert_eq!(present, expect_present);
    }
}
