//! Match pipeline elements by plugin, element kind, element name, etc.

use thiserror::Error;

use super::{ElementKind, ElementName, OutputName, SourceName, TransformName};

/// Matches some elements of the pipeline based on their type and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementNamePattern {
    /// The kind of element that is matched.
    /// `None` means that any kind of element is accepted.
    pub kind: Option<ElementKind>,
    /// A pattern that matches some plugin names.
    pub plugin: StringPattern,
    /// A pattern that matches some element names.
    pub element: StringPattern,
}

/// A pattern that matches a name (String).
///
/// Name patterns are a very simplified form of regular expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringPattern {
    Exact(String),
    StartWith(String),
    EndWith(String),
    Any,
}

// Below are "restricted" patterns that only work on a specific element kind.
// They can only be created if it can be proved that the kind to be matched is the right one.

/// Matches some source names.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceNamePattern(ElementNamePattern);

/// Matches some transform names.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformNamePattern(ElementNamePattern);
/// Matches some output names.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputNamePattern(ElementNamePattern);

#[derive(Debug, Error)]
#[error("incompatible element kind: expected {expected}, was {actual}")]
pub struct IncompatibleKindError {
    expected: ElementKind,
    actual: ElementKind,
}

impl StringPattern {
    pub fn matches(&self, name: &str) -> bool {
        match self {
            StringPattern::Exact(pat) => pat == name,
            StringPattern::StartWith(pat) => name.starts_with(pat),
            StringPattern::EndWith(pat) => name.ends_with(pat),
            StringPattern::Any => true,
        }
    }
}

impl ElementNamePattern {
    /// Creates a "wildcard" pattern that matches everything.
    pub fn wildcard() -> Self {
        Self {
            kind: None,
            plugin: StringPattern::Any,
            element: StringPattern::Any,
        }
    }

    /// Checks whether this pattern matches the given name.
    ///
    /// To match, every part of the name must be accepted by the pattern:
    /// its kind, plugin name and element name.
    ///
    /// # Example
    /// ```
    /// use alumet::pipeline::naming::{ElementKind, ElementName};
    /// use alumet::pipeline::naming::matching::{ElementNamePattern, StringPattern};
    ///
    /// let name1 = ElementName {
    ///     kind: ElementKind::Source,
    ///     plugin: String::from("test"),
    ///     element: String::from("example-source"),
    /// };
    /// let name2 = ElementName {
    ///     kind: ElementKind::Source,
    ///     plugin: String::from("test"),
    ///     element: String::from("other-source"),
    /// };
    ///
    /// let pattern = ElementNamePattern {
    ///     kind: Some(ElementKind::Source),
    ///     plugin: StringPattern::Exact(String::from("test")),
    ///     element: StringPattern::StartWith(String::from("example-")),
    /// };
    /// assert!(pattern.matches(&name1));
    /// assert!(!pattern.matches(&name2));
    /// ```
    pub fn matches<'a, N: Into<&'a ElementName>>(&'a self, name: N) -> bool {
        let name = name.into();
        let kind_matches = match self.kind {
            None => true,
            Some(k) if k == name.kind => true,
            _ => false,
        };
        kind_matches && self.plugin.matches(&name.plugin) && self.element.matches(&name.element)
    }
}

impl SourceNamePattern {
    pub fn new(plugin: StringPattern, source: StringPattern) -> Self {
        Self(ElementNamePattern {
            kind: Some(ElementKind::Source),
            plugin,
            element: source,
        })
    }

    pub fn exact<S: Into<String>>(plugin: S, source: S) -> Self {
        Self::new(StringPattern::Exact(plugin.into()), StringPattern::Exact(source.into()))
    }

    /// Creates a "wildcard" pattern that matches all sources.
    pub fn wildcard() -> Self {
        Self::new(StringPattern::Any, StringPattern::Any)
    }

    pub fn matches(&self, name: &SourceName) -> bool {
        self.0.plugin.matches(&name.0.plugin) && self.0.element.matches(&name.0.element)
    }
}

impl TransformNamePattern {
    pub fn new(plugin: StringPattern, transform: StringPattern) -> Self {
        Self(ElementNamePattern {
            kind: Some(ElementKind::Transform),
            plugin,
            element: transform,
        })
    }

    pub fn exact<S: Into<String>>(plugin: S, source: S) -> Self {
        Self::new(StringPattern::Exact(plugin.into()), StringPattern::Exact(source.into()))
    }

    /// Creates a "wildcard" pattern that matches all transforms.
    pub fn wildcard() -> Self {
        Self::new(StringPattern::Any, StringPattern::Any)
    }

    pub fn matches(&self, name: &TransformName) -> bool {
        self.0.plugin.matches(&name.0.plugin) && self.0.element.matches(&name.0.element)
    }
}

impl OutputNamePattern {
    pub fn new(plugin: StringPattern, output: StringPattern) -> Self {
        Self(ElementNamePattern {
            kind: Some(ElementKind::Output),
            plugin,
            element: output,
        })
    }

    pub fn exact<S: Into<String>>(plugin: S, source: S) -> Self {
        Self::new(StringPattern::Exact(plugin.into()), StringPattern::Exact(source.into()))
    }

    /// Creates a "wildcard" pattern that matches all outputs.
    pub fn wildcard() -> Self {
        Self::new(StringPattern::Any, StringPattern::Any)
    }

    pub fn matches(&self, name: &OutputName) -> bool {
        self.0.plugin.matches(&name.0.plugin) && self.0.element.matches(&name.0.element)
    }
}

// ===== Conversion from/to SourceNamePattern

impl From<SourceNamePattern> for ElementNamePattern {
    fn from(value: SourceNamePattern) -> Self {
        value.0
    }
}

impl From<SourceName> for SourceNamePattern {
    fn from(value: SourceName) -> Self {
        SourceNamePattern::exact(value.0.plugin, value.0.element)
    }
}

impl From<&SourceName> for SourceNamePattern {
    fn from(value: &SourceName) -> Self {
        SourceNamePattern::exact(value.0.plugin.to_owned(), value.0.element.to_owned())
    }
}

impl TryFrom<ElementNamePattern> for SourceNamePattern {
    type Error = IncompatibleKindError;

    fn try_from(mut value: ElementNamePattern) -> Result<Self, Self::Error> {
        match value.kind {
            None | Some(ElementKind::Source) => {
                value.kind = Some(ElementKind::Source);
                Ok(SourceNamePattern(value))
            }
            Some(bad) => Err(IncompatibleKindError {
                expected: ElementKind::Source,
                actual: bad,
            }),
        }
    }
}

impl SourceNamePattern {
    /// If this pattern is guaranteed to only match one source, returns the corresponding `SourceName`.
    pub fn into_single_name(self) -> Option<SourceName> {
        match (self.0.plugin, self.0.element) {
            (StringPattern::Exact(plugin), StringPattern::Exact(source)) => Some(SourceName::new(plugin, source)),
            _ => None,
        }
    }
}

// ===== Conversion from/to TransformNamePattern

impl From<TransformNamePattern> for ElementNamePattern {
    fn from(value: TransformNamePattern) -> Self {
        value.0
    }
}

impl TryFrom<ElementNamePattern> for TransformNamePattern {
    type Error = IncompatibleKindError;

    fn try_from(mut value: ElementNamePattern) -> Result<Self, Self::Error> {
        match value.kind {
            None | Some(ElementKind::Transform) => {
                value.kind = Some(ElementKind::Transform);
                Ok(TransformNamePattern(value))
            }
            Some(bad) => Err(IncompatibleKindError {
                expected: ElementKind::Transform,
                actual: bad,
            }),
        }
    }
}

impl TransformNamePattern {
    /// If this pattern is guaranteed to only match one transform, returns the corresponding `TransformName`.
    pub fn into_single_name(self) -> Option<TransformName> {
        match (self.0.plugin, self.0.element) {
            (StringPattern::Exact(plugin), StringPattern::Exact(trans)) => Some(TransformName::new(plugin, trans)),
            _ => None,
        }
    }
}

// ===== Conversion from/to OutputNamePattern

impl From<OutputNamePattern> for ElementNamePattern {
    fn from(value: OutputNamePattern) -> Self {
        value.0
    }
}

impl TryFrom<ElementNamePattern> for OutputNamePattern {
    type Error = IncompatibleKindError;

    fn try_from(mut value: ElementNamePattern) -> Result<Self, Self::Error> {
        match value.kind {
            None | Some(ElementKind::Output) => {
                value.kind = Some(ElementKind::Output);
                Ok(OutputNamePattern(value))
            }
            Some(bad) => Err(IncompatibleKindError {
                expected: ElementKind::Output,
                actual: bad,
            }),
        }
    }
}

impl OutputNamePattern {
    /// If this pattern is guaranteed to only match one transform, returns the corresponding `TransformName`.
    pub fn into_single_name(self) -> Option<OutputName> {
        match (self.0.plugin, self.0.element) {
            (StringPattern::Exact(plugin), StringPattern::Exact(out)) => Some(OutputName::new(plugin, out)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::pipeline::matching::{
        ElementNamePattern, OutputNamePattern, SourceNamePattern, StringPattern, TransformNamePattern,
    };
    use crate::pipeline::naming::ElementKind;

    #[test]
    fn convert_generic_wildcard_to_specific() {
        assert_eq!(
            SourceNamePattern::try_from(ElementNamePattern::wildcard()).unwrap(),
            SourceNamePattern::wildcard()
        );
        assert_eq!(
            TransformNamePattern::try_from(ElementNamePattern::wildcard()).unwrap(),
            TransformNamePattern::wildcard()
        );
        assert_eq!(
            OutputNamePattern::try_from(ElementNamePattern::wildcard()).unwrap(),
            OutputNamePattern::wildcard()
        );
    }

    #[test]
    fn convert_speficic_wildcard_to_generic() {
        assert_eq!(
            ElementNamePattern::try_from(SourceNamePattern::wildcard()).unwrap(),
            ElementNamePattern {
                kind: Some(ElementKind::Source),
                plugin: StringPattern::Any,
                element: StringPattern::Any
            }
        );
        assert_eq!(
            ElementNamePattern::try_from(TransformNamePattern::wildcard()).unwrap(),
            ElementNamePattern {
                kind: Some(ElementKind::Transform),
                plugin: StringPattern::Any,
                element: StringPattern::Any
            }
        );
        assert_eq!(
            ElementNamePattern::try_from(OutputNamePattern::wildcard()).unwrap(),
            ElementNamePattern {
                kind: Some(ElementKind::Output),
                plugin: StringPattern::Any,
                element: StringPattern::Any
            }
        );
    }
}
