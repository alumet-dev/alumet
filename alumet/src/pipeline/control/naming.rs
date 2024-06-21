use std::collections::HashMap;

/// Generates names for the pipeline elements.
pub(crate) struct NameGenerator {
    existing_names: HashMap<String, usize>,
}

impl NameGenerator {
    pub fn new() -> Self {
        Self {
            existing_names: HashMap::new(),
        }
    }

    pub fn deduplicate(&mut self, mut name: String, always_suffix: bool) -> String {
        use std::fmt::Write;

        match self.existing_names.get_mut(&name) {
            Some(n) => {
                *n += 1;
                write!(name, "-{n}").unwrap();
            }
            None => {
                self.existing_names.insert(name.clone(), 0);
                if always_suffix {
                    write!(name, "-0").unwrap();
                }
            }
        }
        name
    }
}

impl Default for NameGenerator {
    fn default() -> Self {
        Self::new()
    }
}
