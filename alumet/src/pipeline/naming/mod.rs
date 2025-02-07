use std::fmt::Display;

pub mod dedup;
pub mod key;

pub(crate) use dedup::NameDeduplicator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Source,
    Transform,
    Output,
}

/// The name of a pipeline element (source, transform, output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementName {
    /// Name of the plugin that registered this element.
    plugin: String,
    /// Which type of element this is.
    kind: ElementKind,
    /// Name of the element.
    name: String,
}

pub struct SourceName(ElementName);
pub struct TransformName(ElementName);
pub struct OutputName(ElementName);

impl ElementName {
    pub fn as_source(self) -> Option<SourceName> {
        match self.kind {
            ElementKind::Source => Some(SourceName(self)),
            _ => None,
        }
    }

    pub fn as_transform(self) -> Option<TransformName> {
        match self.kind {
            ElementKind::Transform => Some(TransformName(self)),
            _ => None,
        }
    }

    pub fn as_output(self) -> Option<OutputName> {
        match self.kind {
            ElementKind::Output => Some(OutputName(self)),
            _ => None,
        }
    }
}

impl From<SourceName> for ElementName {
    fn from(value: SourceName) -> Self {
        value.0
    }
}

impl From<TransformName> for ElementName {
    fn from(value: TransformName) -> Self {
        value.0
    }
}

impl From<OutputName> for ElementName {
    fn from(value: OutputName) -> Self {
        value.0
    }
}

impl Display for ElementKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ElementKind::Source => "source",
            ElementKind::Transform => "transform",
            ElementKind::Output => "output",
        };
        f.write_str(s)
    }
}

impl Display for ElementName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}/{}", self.kind, self.plugin, self.name)
    }
}

impl Display for SourceName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Display for TransformName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Display for OutputName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
