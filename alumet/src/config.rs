//! Configuration structures and utilities.
//!
//! This module defines the configuration structures that can be passed to plugins
//! during their initialization. It provides an abstraction over specific structures
//! provided by underlying libraries like serde_json or toml.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt::Display,
};

#[derive(Debug)]
pub enum ConfigValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    // Datetime(ConfigDatetime), not supported yet
    Array(ConfigArray),
    Table(ConfigTable),
}

#[derive(Debug)]
pub struct ConfigTable {
    content: BTreeMap<String, ConfigValue>,
}

#[derive(Debug)]
pub struct ConfigArray {
    content: Vec<ConfigValue>,
}

#[derive(Debug)]
pub enum ConfigError {
    UnsupportedType { data_type: &'static str },
}

impl Error for ConfigError {}

impl Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot create a FFI-compatible version of the configuration: ")?;
        match self {
            ConfigError::UnsupportedType { data_type } => write!(f, "unsupported data type: {data_type}"),
        }
    }
}

impl ConfigTable {
    /// Consumes a toml `Table` to create a C-compatible `ConfigTable`.
    /// Every `String` is replaced by a `CString` with a 0 byte at the end.
    pub fn new(table: toml::Table) -> Result<ConfigTable, ConfigError> {
        /// Converts a table recursively.
        fn convert_table(t: toml::Table) -> Result<ConfigTable, ConfigError> {
            let mut content = BTreeMap::new();
            for (key, value) in t {
                content.insert(key, convert(value)?);
            }
            Ok(ConfigTable { content })
        }
        /// Converts a toml Value.
        fn convert(value: toml::Value) -> Result<ConfigValue, ConfigError> {
            match value {
                toml::Value::String(str) => Ok(ConfigValue::String(str)),
                toml::Value::Integer(v) => Ok(ConfigValue::Integer(v)),
                toml::Value::Float(v) => Ok(ConfigValue::Float(v)),
                toml::Value::Boolean(v) => Ok(ConfigValue::Boolean(v)),
                toml::Value::Datetime(_) => Err(ConfigError::UnsupportedType { data_type: "datetime" })?,
                toml::Value::Array(arr) => {
                    let content: Result<Vec<ConfigValue>, ConfigError> = arr.into_iter().map(|v| convert(v)).collect();
                    Ok(ConfigValue::Array(ConfigArray { content: content? }))
                }
                toml::Value::Table(t) => Ok(ConfigValue::Table(convert_table(t)?)),
            }
        }
        convert_table(table)
    }

    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.content.get(key)
    }

    pub fn len(&self) -> usize {
        self.content.len()
    }
}

impl ConfigArray {
    pub fn get(&self, index: usize) -> Option<&ConfigValue> {
        self.content.get(index)
    }

    pub fn len(&self) -> usize {
        self.content.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrong_data() {
        let table1 = toml::toml! {
            "\0" = "bad_key"
        };
        let table2 = toml::toml! {
            "bad_value" = ["a\0b"]
        };
        let table3 = toml::toml! {
            [subtable]
            "bad_key\0" = "ab"
        };

        for (table, nul_pos_in_str) in vec![(table1, 0), (table2, 1), (table3, 7)] {
            let ffi_table = ConfigTable::new(table);
            match ffi_table {
                _ => panic!("ConfigTable::new should have failed on invalid data"),
            }
        }
    }
}
