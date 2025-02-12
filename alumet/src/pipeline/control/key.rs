//! Keys associated to individual elements.

use crate::pipeline::naming::{ElementKind, ElementName, OutputName, SourceName, TransformName};

// The inner field is private because it could be replaced by an integer in the future
// in order to reduce the size of the key and improve performance.

#[derive(Debug, Clone, PartialEq)]
pub struct SourceKey(pub(super) SourceName);
#[derive(Debug, Clone, PartialEq)]
pub struct TransformKey(pub(super) TransformName);
#[derive(Debug, Clone, PartialEq)]
pub struct OutputKey(pub(super) OutputName);

impl SourceKey {
    pub(crate) fn new(name: SourceName) -> Self {
        Self(name)
    }
}

impl TransformKey {
    pub(crate) fn new(name: TransformName) -> Self {
        Self(name)
    }
}

impl OutputKey {
    pub(crate) fn new(name: OutputName) -> Self {
        Self(name)
    }
}
