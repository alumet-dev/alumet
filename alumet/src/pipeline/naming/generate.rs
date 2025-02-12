//! Automatic name generation.

use std::{
    borrow::Cow,
    sync::atomic::{AtomicU32, Ordering},
};

/// Generates unique names with a prefix.
///
/// # Uniqueness
/// Every name returned by the generator is unique, in that it will never return the same name twice.
/// It is your responsibility to ensure that you **do not give the same prefix to different generators**.
///
/// # Thread safety
/// `NameGenerator` is [`Send`] and [`Sync`]: it can be sent to threads and shared between threads safely.
/// You may need to wrap the generator in an [`Arc`].
///
/// # Example
///
/// ```
/// use alumet::pipeline::naming::generate::NameGenerator;
///
/// let namegen = NameGenerator::new("prefix");
/// assert_eq!(namegen.next_name(), "prefix-0");
/// assert_eq!(namegen.next_name(), "prefix-1");
/// ```
///
pub struct NameGenerator {
    prefix: Cow<'static, str>,
    counter: AtomicU32,
}

impl NameGenerator {
    /// Creates a new generator with a statically known prefix.
    pub const fn new(prefix: &'static str) -> Self {
        Self {
            prefix: Cow::Borrowed(prefix), // no copy
            counter: AtomicU32::new(0),
        }
    }

    /// Creates a new generator with the given prefix.
    pub fn with_prefix_slice(prefix: &str) -> Self {
        Self {
            prefix: Cow::Owned(prefix.to_owned()), // copy
            counter: AtomicU32::new(0),
        }
    }

    /// Creates a new generator with the given prefix.
    pub fn with_prefix_owned(prefix: String) -> Self {
        Self {
            prefix: Cow::Owned(prefix), // no copy
            counter: AtomicU32::new(0),
        }
    }

    /// Generates a new name of the form `{prefix}-{n}` where n is unique.
    pub fn next_name(&self) -> String {
        let prefix = &self.prefix;
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{prefix}-{n}")
    }

    /// Generates a new name of the form `{prefix}-{n}-{addendum}` where n is unique.
    pub fn custom_name(&self, addendum: &str) -> String {
        let prefix = &self.prefix;
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{prefix}-{n}-{addendum}")
    }
}

#[cfg(test)]
mod tests {
    use crate::pipeline::util::{assert_send, assert_sync};

    use super::NameGenerator;

    #[test]
    fn type_properties() {
        assert_send::<NameGenerator>();
        assert_sync::<NameGenerator>();
    }

    #[test]
    fn unique_names() {
        let g = NameGenerator::new("test");
        assert_eq!(g.next_name(), "test-0");
        assert_eq!(g.next_name(), "test-1");
        assert_eq!(g.next_name(), "test-2");
        assert_eq!(g.custom_name("wow"), "test-3-wow");
        assert_eq!(g.custom_name("wow"), "test-4-wow");
        assert_eq!(g.next_name(), "test-5");
    }
}
