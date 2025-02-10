//! Match pipeline elements by plugin, element kind, element name, etc.

use thiserror::Error;

use super::{ElementKind, ElementName, OutputName, SourceName, TransformName};

// use super::naming::{ElementKind, ElementName, ElementNameParts, OutputName, SourceName, TransformName};

/// Matches some elements of the pipeline based on their type and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementMatcher {
    /// The kind of element that is matched.
    /// `None` means that any kind of element is accepted.
    pub kind: Option<ElementKind>,
    /// A pattern that matches some plugin names.
    pub plugin: NamePattern,
    /// A pattern that matches some element names.
    pub element: NamePattern,
}

/// A pattern that matches a name (String).
///
/// Name patterns are a very simplified form of regular expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamePattern {
    Exact(String),
    StartWith(String),
    EndWith(String),
    Any,
}

// Below are "restricted" matchers that only work on a specific element kind.
// They can only be created if it can be proved that the kind to be matched is the right one.

/// Matches some sources based on their name.
#[derive(Debug, Clone)]
pub struct SourceMatcher(ElementMatcher);

/// Matches some transforms based on their name.
#[derive(Debug, Clone)]
pub struct TransformMatcher(ElementMatcher);
/// Matches some outputs based on their name.
#[derive(Debug, Clone)]
pub struct OutputMatcher(ElementMatcher);

#[derive(Debug, Error)]
#[error("incompatible element kind: expected {expected}, was {actual}")]
pub struct IncompatibleKindError {
    expected: ElementKind,
    actual: ElementKind,
}

impl NamePattern {
    pub fn matches(&self, name: &str) -> bool {
        match self {
            NamePattern::Exact(pat) => pat == name,
            NamePattern::StartWith(pat) => name.starts_with(pat),
            NamePattern::EndWith(pat) => name.ends_with(pat),
            NamePattern::Any => true,
        }
    }
}

impl ElementMatcher {
    /// Checks whether this matcher matches the given name.
    ///
    /// To match, every part of the name must be accepted by the matcher:
    /// its kind, plugin name and element name.
    ///
    /// # Example
    /// ```
    /// let name1 = ElementName {
    ///     kind: ElementKind::Source,
    ///     plugin: "test",
    ///     element: "example-source",
    /// };
    /// let name2 = ElementName {
    ///     kind: ElementKind::Source,
    ///     plugin: "test",
    ///     element: "other-source",
    /// };
    ///
    /// let matcher = ElementMatcher {
    ///     kind: Some(ElementKind::Source),
    ///     plugin: NamePattern::Exact(String::from("test")),
    ///     element: NamePattern::StartWith(String::from("example-")),
    /// };
    /// assert!(matcher.matches(&name));
    /// assert!(!matcher.matches(&name));
    /// ```
    pub fn matches(&self, name: &ElementName) -> bool {
        let kind_matches = match self.kind {
            None => true,
            Some(k) if k == name.kind => true,
            _ => false,
        };
        kind_matches && self.plugin.matches(&name.plugin) && self.element.matches(&name.element)
    }
}

impl SourceMatcher {
    pub fn new(plugin: NamePattern, source: NamePattern) -> Self {
        Self(ElementMatcher {
            kind: Some(ElementKind::Source),
            plugin,
            element: source,
        })
    }

    /// Creates a "wildcard" matcher that matches all sources.
    pub fn wildcard() -> Self {
        Self::new(NamePattern::Any, NamePattern::Any)
    }

    pub fn matches(&self, name: &SourceName) -> bool {
        self.0.plugin.matches(&name.0.plugin) && self.0.element.matches(&name.0.element)
    }
}

impl TransformMatcher {
    pub fn new(plugin: NamePattern, transform: NamePattern) -> Self {
        Self(ElementMatcher {
            kind: Some(ElementKind::Transform),
            plugin,
            element: transform,
        })
    }

    /// Creates a "wildcard" matcher that matches all transforms.
    pub fn wildcard() -> Self {
        Self::new(NamePattern::Any, NamePattern::Any)
    }

    pub fn matches(&self, name: &TransformName) -> bool {
        self.0.plugin.matches(&name.0.plugin) && self.0.element.matches(&name.0.element)
    }
}

impl OutputMatcher {
    pub fn new(plugin: NamePattern, output: NamePattern) -> Self {
        Self(ElementMatcher {
            kind: Some(ElementKind::Output),
            plugin,
            element: output,
        })
    }

    /// Creates a "wildcard" matcher that matches all outputs.
    pub fn wildcard() -> Self {
        Self::new(NamePattern::Any, NamePattern::Any)
    }

    pub fn matches(&self, name: &OutputName) -> bool {
        self.0.plugin.matches(&name.0.plugin) && self.0.element.matches(&name.0.element)
    }
}

// ===== Conversion from/to SourceMatcher

impl From<SourceMatcher> for ElementMatcher {
    fn from(value: SourceMatcher) -> Self {
        value.0
    }
}

impl TryFrom<ElementMatcher> for SourceMatcher {
    type Error = IncompatibleKindError;

    fn try_from(value: ElementMatcher) -> Result<Self, Self::Error> {
        match value.kind {
            None | Some(ElementKind::Source) => Ok(SourceMatcher(value)),
            Some(bad) => Err(IncompatibleKindError {
                expected: ElementKind::Source,
                actual: bad,
            }),
        }
    }
}

impl SourceMatcher {
    /// If this matcher is guaranteed to only match one source, returns the corresponding `SourceName`.
    pub fn into_single_name(self) -> Option<SourceName> {
        match (self.0.plugin, self.0.element) {
            (NamePattern::Exact(plugin), NamePattern::Exact(source)) => Some(SourceName::new(plugin, source)),
            _ => None,
        }
    }
}

// ===== Conversion from/to TransformMatcher

impl From<TransformMatcher> for ElementMatcher {
    fn from(value: TransformMatcher) -> Self {
        value.0
    }
}

impl TryFrom<ElementMatcher> for TransformMatcher {
    type Error = IncompatibleKindError;

    fn try_from(value: ElementMatcher) -> Result<Self, Self::Error> {
        match value.kind {
            None | Some(ElementKind::Transform) => Ok(TransformMatcher(value)),
            Some(bad) => Err(IncompatibleKindError {
                expected: ElementKind::Transform,
                actual: bad,
            }),
        }
    }
}

impl TransformMatcher {
    /// If this matcher is guaranteed to only match one transform, returns the corresponding `TransformName`.
    pub fn into_single_name(self) -> Option<TransformName> {
        match (self.0.plugin, self.0.element) {
            (NamePattern::Exact(plugin), NamePattern::Exact(trans)) => Some(TransformName::new(plugin, trans)),
            _ => None,
        }
    }
}

// ===== Conversion from/to OutputMatcher

impl From<OutputMatcher> for ElementMatcher {
    fn from(value: OutputMatcher) -> Self {
        value.0
    }
}

impl TryFrom<ElementMatcher> for OutputMatcher {
    type Error = IncompatibleKindError;

    fn try_from(value: ElementMatcher) -> Result<Self, Self::Error> {
        match value.kind {
            None | Some(ElementKind::Output) => Ok(OutputMatcher(value)),
            Some(bad) => Err(IncompatibleKindError {
                expected: ElementKind::Output,
                actual: bad,
            }),
        }
    }
}

impl OutputMatcher {
    /// If this matcher is guaranteed to only match one transform, returns the corresponding `TransformName`.
    pub fn into_single_name(self) -> Option<OutputName> {
        match (self.0.plugin, self.0.element) {
            (NamePattern::Exact(plugin), NamePattern::Exact(out)) => Some(OutputName::new(plugin, out)),
            _ => None,
        }
    }
}

/// Parse strings into matching structures.
mod parsing {
    use std::str::FromStr;
    use thiserror::Error;

    use crate::pipeline::naming::ElementKind;

    use super::NamePattern;

    /// Parses a string to an `ElementKind`.
    ///
    /// `"*"`, `"all"` and `"any"` parse to `None`, which indicate that any kind of element is
    /// accepted by the [`ElementMatcher`].
    pub fn parse_kind(kind: &str) -> Result<Option<ElementKind>, KindParseError> {
        match kind {
            "src" | "source" | "sources" => Ok(Some(ElementKind::Source)),
            "tra" | "transform" | "transforms" => Ok(Some(ElementKind::Transform)),
            "out" | "output" | "outputs" => Ok(Some(ElementKind::Output)),
            "*" | "all" | "any" => Ok(None),
            _ => Err(KindParseError),
        }
    }

    #[derive(Debug)]
    pub struct KindParseError;

    #[derive(Debug, Error, PartialEq, Eq)]
    pub enum NamePatternParseError {
        #[error("Invalid pattern: asterisk '*' in the middle of the string")]
        Asterisk,
        #[error("Invalid pattern: the string is empty")]
        Empty,
    }

    impl FromStr for NamePattern {
        type Err = NamePatternParseError;

        /// Parses a `NamePattern`.
        ///
        /// The only special character in name patterns is `*`, which acts as a "wildcard".
        /// For instance, `a*` matches every name that begins with `a`.
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            if s.is_empty() {
                Err(NamePatternParseError::Empty)
            } else if s == "*" {
                Ok(NamePattern::Any)
            } else if let Some(suffix) = s.strip_prefix('*') {
                if suffix.contains('*') {
                    Err(NamePatternParseError::Asterisk)
                } else {
                    Ok(NamePattern::EndWith(suffix.to_owned()))
                }
            } else if let Some(prefix) = s.strip_suffix('*') {
                if prefix.contains('*') {
                    Err(NamePatternParseError::Asterisk)
                } else {
                    Ok(NamePattern::StartWith(prefix.to_owned()))
                }
            } else {
                if s.contains('*') {
                    Err(NamePatternParseError::Asterisk)
                } else {
                    Ok(NamePattern::Exact(s.to_owned()))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::pipeline::naming::matching::{parsing::NamePatternParseError, NamePattern};

    #[test]
    fn parse_name_pattern() -> anyhow::Result<()> {
        assert_eq!(NamePattern::from_str("*")?, NamePattern::Any);
        assert_eq!(NamePattern::from_str("*abcd")?, NamePattern::EndWith("abcd".to_owned()));
        assert_eq!(
            NamePattern::from_str("abcd*")?,
            NamePattern::StartWith("abcd".to_owned())
        );
        assert_eq!(NamePattern::from_str("exact")?, NamePattern::Exact("exact".to_owned()));
        assert_eq!(NamePattern::from_str("a*b"), Err(NamePatternParseError::Asterisk));
        assert_eq!(NamePattern::from_str("a*b*c"), Err(NamePatternParseError::Asterisk));
        assert_eq!(NamePattern::from_str(""), Err(NamePatternParseError::Empty));
        Ok(())
    }
}
