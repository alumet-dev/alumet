//! (De)serialization support for Regex.

use regex::Regex;
use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S>(regex: &Regex, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&regex.to_string())
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<Regex, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let regex = Regex::new(&s).unwrap();
    Ok(regex)
}

pub mod option {
    use regex::Regex;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(opt: &Option<Regex>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match opt {
            Some(regex) => super::serialize(regex, serializer),
            None => serializer.serialize_str(""),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Regex>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.is_empty() {
            Ok(None)
        } else {
            let regex = Regex::new(&s).unwrap();
            Ok(Some(regex))
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use regex::Regex;
    use serde::{Deserialize, Serialize};
    use toml;

    #[derive(Debug, Serialize, Deserialize)]
    struct SerdeRegex {
        #[serde(with = "super")]
        inner: Regex,
    }

    fn test_regex_serde(regex_string: &str, expected_toml: &str) {
        let s = SerdeRegex {
            inner: Regex::new(regex_string).unwrap(),
        };
        let serialized = toml::to_string(&s).unwrap();
        assert_eq!(serialized, expected_toml, "unexpected serialization result");
        let deserialized: SerdeRegex = toml::from_str(&serialized).unwrap();
        let reserialized = toml::to_string(&deserialized).unwrap();
        assert_eq!(serialized, reserialized, "ser/de of Regex should be idempotent");
    }

    #[test]
    fn serde() {
        test_regex_serde("[a-zA-Z0-9]+", "inner = \"[a-zA-Z0-9]+\"\n");
        test_regex_serde(r"[\w]+://[^/\s?#]+[^\s?#]*", "inner = '[\\w]+://[^/\\s?#]+[^\\s?#]*'\n");
    }
}
