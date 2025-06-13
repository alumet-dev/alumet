use std::str::FromStr;

use alumet::measurement::AttributeValue;
use anyhow::Context;
use regex::Regex;
use thiserror::Error;
use util_cgroups::Cgroup;

use crate::probe::{AugmentedMetrics, Metrics};

/// Personalises the metrics and attributes to use for a cgroup probe.
///
/// Note: personali**s**e is UK spelling, not a typo. Let's not rely on the USA for everything.
pub trait ProbePersonaliser: Clone + Send + 'static {
    fn personalise(&mut self, cgroup: &Cgroup<'_>, metrics: &Metrics) -> AugmentedMetrics;
}

impl<F: FnMut(&Cgroup<'_>, &Metrics) -> AugmentedMetrics + Clone + Send + 'static> ProbePersonaliser for F {
    fn personalise(&mut self, cgroup: &Cgroup<'_>, metrics: &Metrics) -> AugmentedMetrics {
        self(cgroup, metrics)
    }
}

/// Generates measurement attributes based on a regex.
///
/// # Example
/// ```
/// use plugin_cgroup::probe::personalise::RegexAttributesExtrator;
/// use alumet::measurement::AttributeValue;
///
/// # fn f() -> anyhow::Result<()> {
/// let mut extractor = RegexAttributesExtrator::new("^/oar/(?<user>[a-zA-Z]+)_(?<job__u64>[0-9]+)$")?;
/// let attrs = extractor.extract("/oar/raffingu_9000")?;
/// assert_eq!(attrs, vec![
///     (String::from("user"), AttributeValue::String(String::from("raffingu"))),
///     (String::from("job"), AttributeValue::U64(9000)),
/// ]);
/// # Ok(())
/// # }
/// # f().unwrap();
/// ```
#[derive(Clone)]
pub struct RegexAttributesExtrator {
    regex: Regex,
    groups: Vec<Option<GroupSpec>>,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct GroupSpec {
    name: String,
    typ: AttributeType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttributeType {
    UInt,
    String,
}

impl RegexAttributesExtrator {
    pub fn new(regex: &str) -> anyhow::Result<Self> {
        let regex = Regex::new(regex)?;
        let groups: Result<Vec<_>, InvalidGroupSpec> = regex
            .capture_names()
            .skip(1) // the first group is the overall match
            .map(|maybe_name| maybe_name.map(GroupSpec::from_str).transpose())
            .collect();
        let groups = groups?;
        Ok(Self { regex, groups })
    }

    /// If `input` matches the regex, fills `attrs` with the name and value of the capture groups.
    ///
    /// If it does not match the regex, returns `Ok(())` and does not modify the vec.
    /// Capture groups that don't have a name or that don't match are ignored.
    pub fn extract_into(&mut self, input: &str, attrs: &mut Vec<(String, AttributeValue)>) -> anyhow::Result<()> {
        if let Some(cap) = self.regex.captures(input) {
            // Optimistically reserve some space, because most of the time we expect to match all groups.
            attrs.reserve(self.groups.len());

            // The first captured group is always the overall match, skip it.
            for (group_match, group_spec) in cap.iter().skip(1).zip(&self.groups) {
                if let (Some(captured), Some(spec)) = (group_match, group_spec) {
                    // If the capture group matches _and_ the group has a name, turn it into a measurement attribute.
                    let value = captured.as_str();
                    let name = spec.name.to_owned();
                    let attr = spec.typ.create_attr(value)?;
                    attrs.push((name, attr));
                }
            }
        }
        Ok(())
    }

    /// If `input` matches the regex, extracts the name and value of the capture groups.
    ///
    /// If it does not match the regex, returns `Ok` with an empty vec.
    /// Capture groups that don't have a name or that don't match are ignored.
    pub fn extract(&mut self, input: &str) -> anyhow::Result<Vec<(String, AttributeValue)>> {
        let mut attrs = Vec::new();
        self.extract_into(input, &mut attrs)?;
        Ok(attrs)
    }
}

#[derive(Debug, Error)]
#[error("invalid group spec: \"{0}\"")]
struct InvalidGroupSpec(String);

impl FromStr for GroupSpec {
    type Err = InvalidGroupSpec;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (name, typ) = if let Some((name, type_str)) = s.split_once("__") {
            let typ = match type_str.trim_ascii() {
                "u64" | "uint" => AttributeType::UInt,
                "str" | "string" => AttributeType::String,
                _ => return Err(InvalidGroupSpec(s.to_owned())),
            };
            (name, typ)
        } else {
            (s, AttributeType::String)
        };
        Ok(GroupSpec {
            name: name.to_owned(),
            typ,
        })
    }
}

impl AttributeType {
    fn create_attr(self, value: &str) -> anyhow::Result<AttributeValue> {
        match self {
            AttributeType::UInt => {
                let int = value.parse().with_context(|| format!("{value} is not a valid u64"))?;
                Ok(AttributeValue::U64(int))
            }
            AttributeType::String => Ok(AttributeValue::String(value.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn regex_extractor_uint() -> anyhow::Result<()> {
        let mut extractor = RegexAttributesExtrator::new("oar-u(?<user__u64>[0-9]+)-j(?<job__uint>[0-9]+)")?;
        assert_eq!(
            extractor.groups,
            vec![
                Some(GroupSpec {
                    name: String::from("user"),
                    typ: AttributeType::UInt
                }),
                Some(GroupSpec {
                    name: String::from("job"),
                    typ: AttributeType::UInt
                })
            ]
        );

        let mut attrs = Vec::new();
        extractor.extract_into(
            "/oar.slice/oar-u1000.slice/oar-u1000-j45670.slice/oar-uXXX-jYYY-sZZZ.scope",
            &mut attrs,
        )?;
        assert_eq!(
            attrs,
            vec![
                (String::from("user"), AttributeValue::U64(1000)),
                (String::from("job"), AttributeValue::U64(45670)),
            ]
        );

        let mut attrs = Vec::new();
        extractor.extract_into(
            "/oar.slice/oar-uZZZ.slice/oar-uZZZ-jJJJ.slice/oar-u1000-j45670-s13320.scope",
            &mut attrs,
        )?;
        assert_eq!(
            attrs,
            vec![
                (String::from("user"), AttributeValue::U64(1000)),
                (String::from("job"), AttributeValue::U64(45670)),
            ]
        );

        attrs.clear();
        extractor.extract_into(
            "/bad-u1000.slice/u1000-j45670.slice/oar-uXXX-jYYY-sZZZ.scope",
            &mut attrs,
        )?;
        assert_eq!(attrs, vec![]);

        attrs.clear();
        extractor.extract_into("/oar.slice/oar-uOOPS-j", &mut attrs)?;
        assert_eq!(attrs, vec![]);
        Ok(())
    }

    #[test]
    fn regex_extractor_mixed() -> anyhow::Result<()> {
        // use ^ and $ so that we match the whole string
        let mut extractor = RegexAttributesExtrator::new("^.*/name=(?<name__str>[a-zA-Z0-9]+)/(?<leaf>[a-zA-Z]+)$")?;
        assert_eq!(
            extractor.groups,
            vec![
                Some(GroupSpec {
                    name: String::from("name"),
                    typ: AttributeType::String
                }),
                Some(GroupSpec {
                    name: String::from("leaf"),
                    typ: AttributeType::String
                })
            ]
        );

        let mut attrs = Vec::new();
        extractor.extract_into("/mycgroup/name=toto/greenie", &mut attrs)?;
        assert_eq!(
            attrs,
            vec![
                (String::from("name"), AttributeValue::String(String::from("toto"))),
                (String::from("leaf"), AttributeValue::String(String::from("greenie"))),
            ]
        );

        attrs.clear();
        extractor.extract_into("/name=toto/greenie", &mut attrs)?;
        assert_eq!(
            attrs,
            vec![
                (String::from("name"), AttributeValue::String(String::from("toto"))),
                (String::from("leaf"), AttributeValue::String(String::from("greenie"))),
            ]
        );

        attrs.clear();
        extractor.extract_into("/name=toto/greenie/other", &mut attrs)?;
        assert_eq!(attrs, vec![]);
        Ok(())
    }
}
