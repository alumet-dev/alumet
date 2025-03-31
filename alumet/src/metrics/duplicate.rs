use super::Metric;

/// Specifies when a new metric registration is considered to be a duplicate
/// of an existing metric.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateCriteria {
    /// If a metric with the same name already exists, that's a duplicate.
    Strict,
    /// If a metric with the exact same definition exists, ignore the new metric.
    /// If a metric with the same name but a **different** definition exists, that's a duplicate.
    Different,
    /// If a metric with a compatible definition exists, ignore the new metric.
    /// If a metric with the same name but an **incompatible** definition exists, that's a duplicate.
    ///
    /// # Compatible metrics
    /// Two metric definitions are deemed "compatible" if [`are_compatible`] returns `true`.
    Incompatible,
}

#[derive(Clone, Debug)]
pub enum DuplicateReaction {
    Error,
    Rename { suffix: String },
}

/// Checks whether two metric definitions are compatible.
///
/// # Compatible metrics
/// Two metric definitions are compatible if they have the same name, unit and value type.
/// The description is ignored.
///
/// # Transitivity
/// This relation is transitive: `are_compatible(a, b) == are_compatible(b, a)`
pub(super) fn are_compatible(m1: &Metric, m2: &Metric) -> bool {
    m1.name == m2.name && m1.unit == m2.unit && m1.value_type == m2.value_type
}

/// Checks whether two metric definitions are identical, that is, all their fields are equal.
///
/// # Transitivity
/// This relation is transitive: `are_identical(a, b) == are_identical(b, a)`
pub(super) fn are_identical(m1: &Metric, m2: &Metric) -> bool {
    are_compatible(m1, m2) && m1.description == m2.description
}

impl DuplicateCriteria {
    /// Applies the criteria: is `m1` a duplicate of `m2`?
    pub fn are_duplicate(self, m1: &Metric, m2: &Metric) -> bool {
        match self {
            DuplicateCriteria::Strict => m1.name == m2.name,
            DuplicateCriteria::Different => !are_identical(m1, m2),
            DuplicateCriteria::Incompatible => !are_compatible(m1, m2),
        }
    }
}
