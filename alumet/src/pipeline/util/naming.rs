//! Things to deal with names.
use std::{
    collections::HashMap,
    fmt::{self, Display},
};

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
    source_dedup: NameDeduplicator,
    transform_dedup: NameDeduplicator,
    output_dedup: NameDeduplicator,
    listener_dedup: NameDeduplicator,
    plugin: PluginName,
}

impl ScopedNameGenerator {
    pub fn new(plugin: PluginName) -> Self {
        Self {
            source_dedup: NameDeduplicator::new(),
            transform_dedup: NameDeduplicator::new(),
            output_dedup: NameDeduplicator::new(),
            listener_dedup: NameDeduplicator::new(),
            plugin,
        }
    }

    pub fn source_name(&mut self, name: &str) -> SourceName {
        SourceName(ElementNameParts {
            plugin: self.plugin.0.clone(),
            element: self.source_dedup.insert_deduplicate(name.to_owned(), false),
        })
    }

    pub fn transform_name(&mut self, name: &str) -> TransformName {
        TransformName(ElementNameParts {
            plugin: self.plugin.0.clone(),
            element: self.transform_dedup.insert_deduplicate(name.to_owned(), false),
        })
    }

    pub fn output_name(&mut self, name: &str) -> OutputName {
        OutputName(ElementNameParts {
            plugin: self.plugin.0.clone(),
            element: self.output_dedup.insert_deduplicate(name.to_owned(), false),
        })
    }

    pub fn listener_name(&mut self, name: &str) -> ListenerName {
        ListenerName(ElementNameParts {
            plugin: self.plugin.0.clone(),
            element: self.listener_dedup.insert_deduplicate(name.to_owned(), false),
        })
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginName(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementNameParts {
    pub(super) plugin: String,
    pub(super) element: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceName(pub(super) ElementNameParts);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformName(pub(super) ElementNameParts);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputName(pub(super) ElementNameParts);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerName(pub(super) ElementNameParts);

impl fmt::Display for PluginName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for SourceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/source/{}", self.0.plugin, self.0.element)
    }
}

impl fmt::Display for TransformName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/transform/{}", self.0.plugin, self.0.element)
    }
}

impl fmt::Display for OutputName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/output/{}", self.0.plugin, self.0.element)
    }
}

impl fmt::Display for ListenerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/listener/{}", self.0.plugin, self.0.element)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementKind {
    Source,
    Transform,
    Output,
}

pub trait ElementName: Display + Clone {
    fn kind() -> ElementKind;
    fn parts(&self) -> &ElementNameParts;
}

impl ElementName for SourceName {
    fn kind() -> ElementKind {
        ElementKind::Source
    }

    fn parts(&self) -> &ElementNameParts {
        &self.0
    }
}

impl ElementName for TransformName {
    fn kind() -> ElementKind {
        ElementKind::Transform
    }

    fn parts(&self) -> &ElementNameParts {
        &self.0
    }
}

impl ElementName for OutputName {
    fn kind() -> ElementKind {
        ElementKind::Output
    }
    fn parts(&self) -> &ElementNameParts {
        &self.0
    }
}
