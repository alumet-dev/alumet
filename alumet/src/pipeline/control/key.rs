//! Keys associated to individual elements.

use crate::pipeline::naming::{ElementKind, ElementName};

// The inner field is private because it could be replaced by an integer in the future
// in order to reduce the size of the key and improve performance.

pub struct SourceKey(ElementName);
pub struct TransformKey(ElementName);
pub struct OutputKey(ElementName);

impl SourceKey {
    pub(crate) fn new(plugin: String, name: String) -> Self {
        Self(ElementName {
            plugin,
            kind: ElementKind::Source,
            element: name,
        })
    }
}

impl TransformKey {
    pub(crate) fn new(plugin: String, name: String) -> Self {
        Self(ElementName {
            plugin,
            kind: ElementKind::Transform,
            element: name,
        })
    }
}

impl OutputKey {
    pub(crate) fn new(plugin: String, name: String) -> Self {
        Self(ElementName {
            plugin,
            kind: ElementKind::Output,
            element: name,
        })
    }
}
