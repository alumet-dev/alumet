use std::{collections::HashMap, fmt};

/// Deduplicates names for the pipeline elements.
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
        match self.existing_names.get_mut(&name) {
            Some(n) => {
                *n += 1;
                write!(name, "-{n}").unwrap();
            }
            None => {
                self.existing_names.insert(name.clone(), 0);
                if suffix {
                    write!(name, "-0").unwrap();
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

/// Generates names for the pipeline elements.
pub struct ScopedNameGenerator {
    dedup: NameDeduplicator,
    plugin: PluginName,
}

impl ScopedNameGenerator {
    pub fn new(plugin: PluginName) -> Self {
        Self {
            dedup: NameDeduplicator::new(),
            plugin,
        }
    }

    fn element_name(&mut self, kind: &str, name: &str) -> ElementName {
        let deduplicated = self.dedup.insert_deduplicate(format!("{kind}-{name}"), false);
        ElementName {
            plugin: self.plugin.0.clone(),
            element: deduplicated,
        }
    }

    pub fn source_name(&mut self, name: &str) -> SourceName {
        self.element_name("source", name)
    }

    pub fn transform_name(&mut self, name: &str) -> TransformName {
        self.element_name("transform", name)
    }
    pub fn output_name(&mut self, name: &str) -> OutputName {
        self.element_name("output", name)
    }
}

pub struct NameGenerator {
    namegen_by_plugin: HashMap<PluginName, ScopedNameGenerator>,
}

impl NameGenerator {
    pub fn new() -> Self {
        Self {
            namegen_by_plugin: HashMap::new(),
        }
    }

    pub fn namegen_for_scope(&mut self, plugin: &PluginName) -> &mut ScopedNameGenerator {
        self.namegen_by_plugin
            .entry(plugin.clone())
            .or_insert_with(|| ScopedNameGenerator::new(plugin.clone()))
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PluginName(pub String);

#[derive(Clone, PartialEq, Eq)]
pub struct ElementName {
    pub(crate) plugin: String,
    pub(crate) element: String,
}

pub type SourceName = ElementName;
pub type TransformName = ElementName;
pub type OutputName = ElementName;

impl fmt::Display for ElementName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.plugin, self.element)
    }
}
