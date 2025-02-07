//! Name deduplication.

use std::collections::HashMap;

/// Deduplicates strings by autogenerating suffixes.
pub(crate) struct NameDeduplicator {
    existing_names: HashMap<String, usize>,
}

impl NameDeduplicator {
    pub fn new() -> Self {
        Self {
            existing_names: HashMap::new(),
        }
    }

    pub fn insert_deduplicate(&mut self, mut name: String, always_suffix: bool) -> String {
        use std::fmt::Write;

        let suffix = always_suffix || name.is_empty();
        let sep = if name.is_empty() { "" } else { "-" };
        match self.existing_names.get_mut(&name) {
            Some(n) => {
                *n += 1;
                write!(name, "{sep}{n}").unwrap();
            }
            None => {
                self.existing_names.insert(name.clone(), 0);
                if suffix {
                    write!(name, "{sep}0").unwrap();
                }
                self.existing_names.insert(name.clone(), 0);
            }
        }
        name
    }
}

impl Default for NameDeduplicator {
    fn default() -> Self {
        Self::new()
    }
}
