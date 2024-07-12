//! Match pipeline elements by plugin, element kind, element name, etc.

use std::{error::Error, fmt::Display, marker::PhantomData, str::FromStr};

use super::naming::{ElementKind, ElementName, ElementNameParts, OutputName, SourceName, TransformName};

#[derive(Debug)]
pub enum ElementSelector {
    Source(SourceSelector),
    Transform(TransformSelector),
    Output(OutputSelector),
    Any(NamePatterns),
}

#[derive(Debug, Clone)]
pub struct NamePatterns {
    plugin: NamePattern,
    name: NamePattern,
}

#[derive(Debug, Clone)]
pub struct TypedElementSelector<T: ElementName> {
    patterns: NamePatterns,
    t: PhantomData<T>,
}

#[derive(Debug, Clone)]
pub enum NamePattern {
    Exact(String),
    StartWith(String),
    EndWith(String),
    Any,
}

pub type SourceSelector = TypedElementSelector<SourceName>;
pub type TransformSelector = TypedElementSelector<TransformName>;
pub type OutputSelector = TypedElementSelector<OutputName>;

impl From<NamePatterns> for SourceSelector {
    fn from(value: NamePatterns) -> Self {
        SourceSelector::new(value)
    }
}

impl From<NamePatterns> for TransformSelector {
    fn from(value: NamePatterns) -> Self {
        TransformSelector::new(value)
    }
}

impl From<NamePatterns> for OutputSelector {
    fn from(value: NamePatterns) -> Self {
        OutputSelector::new(value)
    }
}

impl NamePatterns {
    pub fn matches<E: ElementName>(&self, name: &E) -> bool {
        self.matches_parts(name.parts())
    }

    fn matches_parts(&self, name_parts: &ElementNameParts) -> bool {
        self.plugin.matches(&name_parts.plugin) && self.name.matches(&name_parts.element)
    }
}

impl ElementSelector {
    fn new(patterns: NamePatterns, kind: Option<ElementKind>) -> Self {
        match kind {
            Some(ElementKind::Source) => ElementSelector::Source(TypedElementSelector::new(patterns)),
            Some(ElementKind::Transform) => ElementSelector::Transform(TypedElementSelector::new(patterns)),
            Some(ElementKind::Output) => ElementSelector::Output(TypedElementSelector::new(patterns)),
            None => ElementSelector::Any(patterns),
        }
    }

    pub fn matches<E: ElementName>(&self, name: &E) -> bool {
        match self {
            ElementSelector::Source(pat) => E::kind() == ElementKind::Source && pat.patterns.matches(name),
            ElementSelector::Transform(pat) => E::kind() == ElementKind::Transform && pat.patterns.matches(name),
            ElementSelector::Output(pat) => E::kind() == ElementKind::Output && pat.patterns.matches(name),
            ElementSelector::Any(patterns) => patterns.matches(name),
        }
    }
}

impl<T: ElementName> TypedElementSelector<T> {
    fn new(patterns: NamePatterns) -> Self {
        Self {
            patterns,
            t: PhantomData,
        }
    }

    pub fn all() -> Self {
        Self {
            patterns: NamePatterns {
                plugin: NamePattern::Any,
                name: NamePattern::Any,
            },
            t: PhantomData,
        }
    }

    pub fn matches(&self, name: &T) -> bool {
        self.patterns.matches(name)
    }
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

#[derive(Debug)]
pub enum SelectorParseError {
    InvalidKind(KindParseError),
    InvalidPluginNamePattern(NamePatternParseError),
    InvalidElementNamePattern(NamePatternParseError),
}

impl Display for SelectorParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}
impl Error for SelectorParseError {}

impl FromStr for ElementSelector {
    type Err = SelectorParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        fn parse_kind(kind: &str) -> Result<Option<ElementKind>, KindParseError> {
            match kind {
                "src" | "source" | "sources" => Ok(Some(ElementKind::Source)),
                "tra" | "transform" | "transforms" => Ok(Some(ElementKind::Transform)),
                "out" | "output" | "outputs" => Ok(Some(ElementKind::Output)),
                "*" | "all" => Ok(None),
                _ => Err(KindParseError),
            }
        }

        let parts: Vec<&str> = s.splitn(3, '/').collect();
        match parts[..] {
            [plugin] => Ok(ElementSelector::Any(NamePatterns {
                plugin: plugin
                    .parse()
                    .map_err(|e| SelectorParseError::InvalidPluginNamePattern(e))?,
                name: NamePattern::Any,
            })),
            [plugin, kind] => {
                let kind = parse_kind(kind).map_err(|e| SelectorParseError::InvalidKind(e))?;
                let patterns = NamePatterns {
                    plugin: plugin
                        .parse()
                        .map_err(|e| SelectorParseError::InvalidPluginNamePattern(e))?,
                    name: NamePattern::Any,
                };
                Ok(ElementSelector::new(patterns, kind))
            }
            [plugin, kind, name] => {
                let kind = parse_kind(kind).map_err(|e| SelectorParseError::InvalidKind(e))?;
                let patterns = NamePatterns {
                    plugin: plugin
                        .parse()
                        .map_err(|e| SelectorParseError::InvalidPluginNamePattern(e))?,
                    name: name
                        .parse()
                        .map_err(|e| SelectorParseError::InvalidElementNamePattern(e))?,
                };
                Ok(ElementSelector::new(patterns, kind))
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
pub struct KindParseError;

#[derive(Debug)]
pub struct NamePatternParseError;

impl FromStr for NamePattern {
    type Err = NamePatternParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "*" {
            Ok(NamePattern::Any)
        } else if let Some(suffix) = s.strip_prefix('*') {
            if suffix.contains('*') {
                Err(NamePatternParseError)
            } else {
                Ok(NamePattern::EndWith(suffix.to_owned()))
            }
        } else if let Some(prefix) = s.strip_suffix('*') {
            if prefix.contains('*') {
                Err(NamePatternParseError)
            } else {
                Ok(NamePattern::StartWith(prefix.to_owned()))
            }
        } else {
            if s.contains('*') {
                Err(NamePatternParseError)
            } else {
                Ok(NamePattern::Exact(s.to_owned()))
            }
        }
    }
}
