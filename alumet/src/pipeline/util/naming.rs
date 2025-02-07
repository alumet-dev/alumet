//! Things to deal with names.
use std::{
    collections::HashMap,
    fmt::{self, Display},
};

use crate::pipeline::naming::NameDeduplicator;

/// Generates names for the pipeline elements of a particular plugin.
pub struct PluginElementNamespace {
    dedup: NameDeduplicator,
    plugin: PluginName,
}

impl PluginElementNamespace {
    pub fn new(plugin: PluginName) -> Self {
        Self {
            dedup: NameDeduplicator::new(),
            plugin,
        }
    }

    pub fn insert_deduplicate(&mut self, name: &str) -> ElementNameParts {
        ElementNameParts {
            plugin: self.plugin.0.clone(),
            element: self.dedup.insert_deduplicate(name.to_owned(), false),
        }
    }
}

pub struct NameGenerator {
    namespaces: HashMap<PluginName, PluginElementNamespace>,
}

impl NameGenerator {
    pub fn new() -> Self {
        Self {
            namespaces: HashMap::new(),
        }
    }

    pub fn plugin_namespace(&mut self, plugin: &PluginName) -> &mut PluginElementNamespace {
        self.namespaces
            .entry(plugin.clone())
            .or_insert_with(|| PluginElementNamespace::new(plugin.clone()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementNameParts {
    pub(super) plugin: String,
    pub(super) element: String,
}

macro_rules! typed_name {
    ($i:ident, $x:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $i(pub(crate) ElementNameParts);

        impl fmt::Display for $i {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}/{}/{}", self.0.plugin, $x, self.0.element)
            }
        }
    };
}

typed_name!(SourceName, "source");
typed_name!(TransformName, "transform");
typed_name!(OutputName, "output");
typed_name!(ListenerName, "listener");

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginName(pub String);

impl fmt::Display for PluginName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
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
