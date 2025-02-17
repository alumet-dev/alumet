use std::str::FromStr;
use thiserror::Error;

use crate::pipeline::naming::ElementKind;

use super::matching::StringPattern;

/// Parses a string to an `ElementKind`.
///
/// `"*"`, `"all"` and `"any"` parse to `None`, which indicate that any kind of element is
/// accepted by the [`ElementPattern`].
pub fn parse_kind(kind: &str) -> Result<Option<ElementKind>, KindParseError> {
    match kind {
        "src" | "source" | "sources" => Ok(Some(ElementKind::Source)),
        "tra" | "transform" | "transforms" => Ok(Some(ElementKind::Transform)),
        "out" | "output" | "outputs" => Ok(Some(ElementKind::Output)),
        "*" | "all" | "any" => Ok(None),
        _ => Err(KindParseError),
    }
}

#[derive(Debug, Error)]
#[error("invalid element kind")]
pub struct KindParseError;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NamePatternParseError {
    #[error("invalid pattern: asterisk '*' in the middle of the string")]
    Asterisk,
    #[error("invalid pattern: the string is empty")]
    Empty,
}

impl FromStr for StringPattern {
    type Err = NamePatternParseError;

    /// Parses a `NamePattern`.
    ///
    /// The only special character in name patterns is `*`, which acts as a "wildcard".
    /// For instance, `a*` matches every name that begins with `a`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            Err(NamePatternParseError::Empty)
        } else if s == "*" {
            Ok(StringPattern::Any)
        } else if let Some(suffix) = s.strip_prefix('*') {
            if suffix.contains('*') {
                Err(NamePatternParseError::Asterisk)
            } else {
                Ok(StringPattern::EndWith(suffix.to_owned()))
            }
        } else if let Some(prefix) = s.strip_suffix('*') {
            if prefix.contains('*') {
                Err(NamePatternParseError::Asterisk)
            } else {
                Ok(StringPattern::StartWith(prefix.to_owned()))
            }
        } else {
            if s.contains('*') {
                Err(NamePatternParseError::Asterisk)
            } else {
                Ok(StringPattern::Exact(s.to_owned()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NamePatternParseError, StringPattern};
    use std::str::FromStr;

    #[test]
    fn parse_name_pattern() -> anyhow::Result<()> {
        assert_eq!(StringPattern::from_str("*")?, StringPattern::Any);
        assert_eq!(
            StringPattern::from_str("*abcd")?,
            StringPattern::EndWith("abcd".to_owned())
        );
        assert_eq!(
            StringPattern::from_str("abcd*")?,
            StringPattern::StartWith("abcd".to_owned())
        );
        assert_eq!(
            StringPattern::from_str("exact")?,
            StringPattern::Exact("exact".to_owned())
        );
        assert_eq!(StringPattern::from_str("a*b"), Err(NamePatternParseError::Asterisk));
        assert_eq!(StringPattern::from_str("a*b*c"), Err(NamePatternParseError::Asterisk));
        assert_eq!(StringPattern::from_str(""), Err(NamePatternParseError::Empty));
        Ok(())
    }
}
