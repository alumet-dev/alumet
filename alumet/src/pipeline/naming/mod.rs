use std::fmt::Display;

pub mod generate;
pub mod matching;
pub mod namespace;

/// The name of a plugin.
///
/// The purpose of this type is to avoid any ambiguity or potential mistake when working with names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginName(pub String);

/// Indicates the type of a pipeline element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Source,
    Transform,
    Output,
}

/// The full name of a pipeline element.
///
/// # Example
/// ```
/// let source_name = ElementName {
///     kind: ElementKind::Source,
///     plugin: String::from("example"),
///     element: String::from("the_source"),
/// };
///
/// // If you know the type (as it is the case here), prefer specialized types:
/// let source_name = SourceName::new(String::from("example"), String::from("the_source"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementName {
    /// Which type of element this is.
    pub kind: ElementKind,
    /// Name of the plugin that registered this element.
    pub plugin: String,
    /// Name of the element.
    pub element: String,
}

/// The full name of a source.
#[derive(Debug, Clone)]
pub struct SourceName(ElementName);

/// The full name of a transform.
#[derive(Debug, Clone)]
pub struct TransformName(ElementName);

/// The full name of an output.
#[derive(Debug, Clone)]
pub struct OutputName(ElementName);

impl SourceName {
    pub fn new(plugin: String, source_name: String) -> Self {
        Self(ElementName {
            plugin,
            kind: ElementKind::Source,
            element: source_name,
        })
    }
}

impl TransformName {
    pub fn new(plugin: String, transform_name: String) -> Self {
        Self(ElementName {
            plugin,
            kind: ElementKind::Transform,
            element: transform_name,
        })
    }
}

impl OutputName {
    pub fn new(plugin: String, output_name: String) -> Self {
        Self(ElementName {
            plugin,
            kind: ElementKind::Output,
            element: output_name,
        })
    }
}

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
        write!(f, "{}/{}/{}", self.kind, self.plugin, self.element)
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
