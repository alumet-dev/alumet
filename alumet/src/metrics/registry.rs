//! Registry of metrics common to the whole pipeline.

use std::collections::HashMap;

use super::{
    def::{Metric, MetricId, RawMetricId},
    duplicate::{self, DuplicateCriteria, DuplicateReaction},
    error::MetricCreationError,
};

/// A registry of metrics.
///
/// New metrics are created by the plugins during their initialization.
#[derive(Clone)]
pub struct MetricRegistry {
    pub(crate) metrics_by_id: HashMap<RawMetricId, Metric>,
    pub(crate) metrics_by_name: HashMap<String, RawMetricId>,
}

impl MetricRegistry {
    /// Creates a new registry, but does not make it "global" yet.
    pub(crate) fn new() -> MetricRegistry {
        MetricRegistry {
            metrics_by_id: HashMap::new(),
            metrics_by_name: HashMap::new(),
        }
    }

    /// Finds the metric that has the given id.
    pub fn by_id<M: MetricId>(&self, id: &M) -> Option<&Metric> {
        self.metrics_by_id.get(&id.untyped_id())
    }

    /// Finds the metric that has the given name.
    pub fn by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)> {
        self.metrics_by_name
            .get(name)
            .and_then(|id| self.metrics_by_id.get(id).map(|m| (*id, m)))
    }

    /// The number of metrics in the registry.
    pub fn len(&self) -> usize {
        self.metrics_by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.metrics_by_id.is_empty()
    }

    /// An iterator on the registered metrics.
    pub fn iter(&self) -> MetricIter<'_> {
        // return new iterator
        MetricIter {
            entries: self.metrics_by_id.iter(),
        }
    }

    /// Generates a new id for a metric and insert it in the registry data structures.
    ///
    /// NOTE: the caller must ensure that the name of the metric is unique.
    fn register_new(&mut self, m: Metric) -> RawMetricId {
        let id = RawMetricId(self.metrics_by_name.len());

        let prev = self.metrics_by_name.insert(m.name.clone(), id);
        debug_assert!(prev.is_none(), "duplicate metric name {}", m.name);

        let prev = self.metrics_by_id.insert(id, m);
        debug_assert!(prev.is_none(), "duplicate metric id {}", id.0);

        id
    }

    /// Registers a new metric in this registry.
    ///
    /// A new id is generated and returned.
    ///
    /// # Duplicates
    /// Metric names are intended to be unique.
    /// If a metric with the same name has already been registered, `register` returns an error.
    pub(crate) fn register(
        &mut self,
        m: Metric,
        dup: DuplicateCriteria,
        on_dup: DuplicateReaction,
    ) -> Result<RawMetricId, MetricCreationError> {
        match on_dup {
            DuplicateReaction::Error => self.register_no_duplicate(m, dup),
            DuplicateReaction::Rename { suffix } => Ok(self.register_infallible(m, dup, &suffix)),
        }
    }

    /// Registers multiple metrics.
    ///
    /// For each metric, the registration may fail if a metric with the same name already exists.
    /// See [`register()`].
    pub(crate) fn register_many(
        &mut self,
        metrics: Vec<Metric>,
        dup: DuplicateCriteria,
        on_dup: DuplicateReaction,
    ) -> Vec<Result<RawMetricId, MetricCreationError>> {
        self.metrics_by_name.reserve(metrics.len());
        self.metrics_by_id.reserve(metrics.len());
        match on_dup {
            DuplicateReaction::Error => metrics
                .into_iter()
                .map(|m| self.register_no_duplicate(m, dup))
                .collect(),
            DuplicateReaction::Rename { suffix } => metrics
                .into_iter()
                .map(|m| Ok(self.register_infallible(m, dup, &suffix)))
                .collect(),
        }
    }

    /// Registers a new metric and deny any duplicate.
    fn register_no_duplicate(
        &mut self,
        m: Metric,
        criteria: DuplicateCriteria,
    ) -> Result<RawMetricId, MetricCreationError> {
        let name = &m.name;
        if let Some(conflict) = self.metrics_by_name.get(name) {
            match criteria {
                DuplicateCriteria::Strict => {
                    return Err(MetricCreationError {
                        name: name.to_owned(),
                        criteria,
                    });
                }
                DuplicateCriteria::Different => {
                    let conflict_def = self.metrics_by_id.get(conflict).unwrap();
                    if !duplicate::are_identical(&m, conflict_def) {
                        return Err(MetricCreationError {
                            name: name.to_owned(),
                            criteria,
                        });
                    }
                }
                DuplicateCriteria::Incompatible => {
                    let conflict_def = self.metrics_by_id.get(conflict).unwrap();
                    if !duplicate::are_compatible(&m, conflict_def) {
                        return Err(MetricCreationError {
                            name: name.to_owned(),
                            criteria,
                        });
                    }
                }
            }
            // At this point, the duplicate criteria has determined that the new metric is identical
            // or compatible with the existing one, and can be ignored.
            // Return the existing metric id.
            Ok(*conflict)
        } else {
            // The metric is not present in the registry, register it and return its (newly generated) id.
            let id = self.register_new(m);
            Ok(id)
        }
    }

    /// Registers a new metric and resolve any conflict to avoid duplicates.
    ///
    /// A new id is generated and returned.
    ///
    /// # Duplicates
    /// Contrary to [`register()`], `register_infallible` does not return an error if a metric with the
    /// same name as `m` already exists in the registry.
    ///
    /// Instead, it:
    /// 1. Checks whether `m` and the conflicting metric are "equal" (same name, same unit, same type of value).
    /// 2. If `m` is different, `register_infallible` uses the `dedup_suffix` to generate a new, unique name for `m`,
    /// and registers it under that name.
    fn register_infallible(&mut self, m: Metric, dup: DuplicateCriteria, dedup_suffix: &str) -> RawMetricId {
        fn resolve_conflict(reg: &mut MetricRegistry, mut metric: Metric, dedup_suffix: &str) -> RawMetricId {
            use std::fmt::Write;

            // Information needed to compare metrics.
            let unit = metric.unit.clone();
            let value_type = metric.value_type.clone();

            // The metric name is modified by this function.
            let mut buf = &mut metric.name;

            // First try: simply append the suffix with an underscore
            write!(&mut buf, "_{dedup_suffix}").expect("dedup_suffix should be writable to metric name");
            match reg.by_name(buf) {
                Some((id, existing)) if existing.unit == unit && existing.value_type == value_type => id,
                Some((_id, _conflict)) => {
                    // Second try: append "_2"
                    buf.push_str("_2");
                    let len_without_n = buf.len() - 1;
                    let mut n = 2;
                    let mut existing = reg.by_name(buf);
                    while existing.is_some() {
                        let (id, other) = existing.unwrap();
                        if other.unit == unit && other.value_type == value_type {
                            // identical to the existing metric, stop here
                            return id;
                        }
                        // n-th try: replace "2" by "{n}"
                        buf.truncate(len_without_n);
                        write!(&mut buf, "{n}").expect("n should be writable to string");
                        n += 1;
                        existing = reg.by_name(buf);
                    }
                    reg.register_new(metric)
                }
                None => reg.register_new(metric),
            }
        }

        let name = &m.name;
        if let Some(conflict_id) = self.metrics_by_name.get(name) {
            let conflict = &self.metrics_by_id[conflict_id];
            if dup.are_duplicate(&m, conflict) {
                // Create a new metric with a slightly different name.
                resolve_conflict(self, m, dedup_suffix)
            } else {
                // Use the existing metric.
                *conflict_id
            }
        } else {
            self.register_new(m)
        }
    }
}

/// An iterator over the metrics of a [`MetricRegistry`].
pub struct MetricIter<'a> {
    entries: std::collections::hash_map::Iter<'a, RawMetricId, Metric>,
}
impl<'a> Iterator for MetricIter<'a> {
    type Item = (&'a RawMetricId, &'a Metric);

    fn next(&mut self) -> Option<Self::Item> {
        self.entries.next()
    }
}

impl<'a> IntoIterator for &'a MetricRegistry {
    type Item = (&'a RawMetricId, &'a Metric);

    type IntoIter = MetricIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        measurement::WrappedMeasurementType,
        metrics::{
            def::Metric,
            duplicate::{DuplicateCriteria, DuplicateReaction},
        },
        units::Unit,
    };

    use super::MetricRegistry;

    #[test]
    fn register_no_duplicate() {
        let mut metrics = MetricRegistry::new();
        assert_eq!(metrics.len(), 0);
        metrics
            .register_no_duplicate(
                Metric {
                    name: "metric".to_owned(),
                    description: "...".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Watt.into(),
                },
                DuplicateCriteria::Strict,
            )
            .unwrap();
        metrics
            .register_no_duplicate(
                Metric {
                    name: "metric".to_owned(),
                    description: "abcd".to_owned(),
                    value_type: WrappedMeasurementType::F64,
                    unit: Unit::Volt.into(),
                },
                DuplicateCriteria::Strict,
            )
            .unwrap_err(); // error is expected
        assert_eq!(metrics.len(), 1);
    }

    #[test]
    fn register() {
        let mut metrics = MetricRegistry::new();
        assert_eq!(metrics.len(), 0);
        let metric_id = metrics
            .register(
                Metric {
                    name: "metric".to_owned(),
                    description: "".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Watt.into(),
                },
                DuplicateCriteria::Strict,
                DuplicateReaction::Error,
            )
            .unwrap();
        let metric_id2 = metrics
            .register(
                Metric {
                    name: "metric2".to_owned(),
                    description: "".to_owned(),
                    value_type: WrappedMeasurementType::F64,
                    unit: Unit::Watt.into(),
                },
                DuplicateCriteria::Strict,
                DuplicateReaction::Error,
            )
            .unwrap();
        let metric_id3 = metrics
            .register(
                Metric {
                    name: "metric2".to_owned(),
                    description: "".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Second.into(),
                },
                DuplicateCriteria::Incompatible,
                DuplicateReaction::Rename {
                    suffix: "dedup".to_owned(),
                },
            )
            .unwrap();
        assert_eq!(metrics.len(), 3);

        let (_id, metric) = metrics.by_name("metric").expect("metric should exist");
        let (_id2, metric2) = metrics.by_name("metric2").expect("metric2 should exist");
        let (_id2, metric3) = metrics.by_name("metric2_dedup").expect("metric2_dedup should exist");
        assert_eq!("metric", metric.name);
        assert_eq!("metric2", metric2.name);
        assert_eq!("metric2_dedup", metric3.name);

        let metric = metrics.by_id(&metric_id).expect("metric should exist");
        let metric2 = metrics.by_id(&metric_id2).expect("metric should exist");
        let metric3 = metrics.by_id(&metric_id3).expect("metric should exist");
        assert_eq!("metric", metric.name);
        assert_eq!("metric2", metric2.name);
        assert_eq!("metric2_dedup", metric3.name);

        let mut names: Vec<&str> = metrics.iter().map(|m| &*m.1.name).collect();
        names.sort();
        assert_eq!(vec!["metric", "metric2", "metric2_dedup"], names);
    }

    #[test]
    fn register_infallible() {
        {
            let mut metrics = MetricRegistry::new();
            assert_eq!(metrics.len(), 0);

            // first registration
            let id1 = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "...".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Watt.into(),
                },
                DuplicateCriteria::Strict,
                "suffix",
            );
            assert_eq!(metrics.len(), 1);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");

            // register again with an identical metric and DuplicateCriteria::Different,
            // the new metric should be ignored
            let id1_bis = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "...".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Watt.into(),
                },
                DuplicateCriteria::Different,
                "suffix",
            );
            assert_eq!(metrics.len(), 1);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(id1, id1_bis);

            // register again with an identical metric and DuplicateCriteria::Strict,
            // a new metric should be registered with a newly generated suffix
            let id1_deduplicated = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "...".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Watt.into(),
                },
                DuplicateCriteria::Strict,
                "suffix",
            );
            assert_eq!(metrics.len(), 2);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(metrics.by_name("metric_suffix").unwrap().1.name, "metric_suffix");
            assert_ne!(id1, id1_deduplicated);

            // register another metric with the same name but a different description,
            // and DuplicateCriteria::Incompatible. It should be ignored.
            let id2 = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "abcd".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Watt.into(),
                },
                DuplicateCriteria::Incompatible,
                "suffix",
            );
            assert_eq!(metrics.len(), 2);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(metrics.by_name("metric_suffix").unwrap().1.name, "metric_suffix");
            assert_eq!(id2, id1);

            // register another metric with an incompatible *type* and DuplicateCriteria::Incompatible.
            // It should create a new, different metric.
            let id3 = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "...".to_owned(),
                    value_type: WrappedMeasurementType::F64,
                    unit: Unit::Watt.into(),
                },
                DuplicateCriteria::Incompatible,
                "suffix",
            );
            assert_eq!(metrics.len(), 3);
            assert_ne!(id3, id1);
            assert_ne!(id3, id2);

            // register another metric with an incompatible *unit* and DuplicateCriteria::Incompatible.
            // It should create a new, different metric.
            let _id4 = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "...".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Byte.into(), // Byte instead of Watt
                },
                DuplicateCriteria::Incompatible,
                "suffix",
            );
            assert_eq!(metrics.len(), 4);

            // register yet another one, which is different
            let _id5 = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "xyz".to_owned(),
                    value_type: WrappedMeasurementType::U64, // U64 instead of F64
                    unit: Unit::Volt.into(),
                },
                DuplicateCriteria::Strict,
                "suffix",
            );
            assert_eq!(metrics.len(), 5);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(metrics.by_name("metric_suffix").unwrap().1.name, "metric_suffix");
            assert_eq!(metrics.by_name("metric_suffix_2").unwrap().1.name, "metric_suffix_2");
            assert_eq!(metrics.by_name("metric_suffix_3").unwrap().1.name, "metric_suffix_3");
            assert_eq!(metrics.by_name("metric_suffix_4").unwrap().1.name, "metric_suffix_4");
            assert_ne!(id3, id2);
            assert_ne!(id3, id1);
            // metric_suffix_1 should NOT exist
            assert!(
                metrics.by_name("metric_suffix_1").is_none(),
                "name generation should go from `suffix` to `suffix_2`"
            );

            // register YET another one, which is different
            let id4 = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "not the same".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Second.into(),
                },
                DuplicateCriteria::Strict,
                "suffix",
            );
            assert_eq!(metrics.len(), 6);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(metrics.by_name("metric_suffix").unwrap().1.name, "metric_suffix");
            assert_eq!(metrics.by_name("metric_suffix_2").unwrap().1.name, "metric_suffix_2");
            assert_eq!(metrics.by_name("metric_suffix_3").unwrap().1.name, "metric_suffix_3");
            assert_eq!(metrics.by_name("metric_suffix_4").unwrap().1.name, "metric_suffix_4");
            assert_eq!(metrics.by_name("metric_suffix_5").unwrap().1.name, "metric_suffix_5");
            assert_ne!(id4, id3);
            assert_ne!(id4, id2);
            assert_ne!(id4, id1);
        }
    }
}
