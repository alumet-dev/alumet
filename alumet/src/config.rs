//! Configuration structures and utilities.
//! 
//! This module defines the configuration structures that can be passed to plugins
//! during their initialization. It provides an abstraction over specific structures
//! provided by underlying libraries like serde_json or toml.

use std::{
    collections::BTreeMap,
    ffi::{c_char, CStr, CString, NulError},
    ptr, fmt::Display, error::Error,
};

#[derive(Debug)]
#[repr(u8)]
pub enum ConfigValue {
    String(CString),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    // Datetime(ConfigDatetime), not supported yet
    Array(ConfigArray),
    Table(ConfigTable),
}

#[derive(Debug)]
pub struct ConfigTable {
    content: BTreeMap<CString, ConfigValue>,
}

#[derive(Debug)]
pub struct ConfigArray {
    content: Vec<ConfigValue>,
}

#[derive(Debug)]
pub enum ConfigError {
    UnsupportedType { data_type: &'static str },
    InvalidNulByte { err: NulError },
}

impl Error for ConfigError {}

impl Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot create a C-compatible version of the configuration: ")?;
        match self {
            ConfigError::UnsupportedType { data_type } => 
                write!(f, "unsupported data type: {data_type}"),
            ConfigError::InvalidNulByte { err } =>
                write!(f, "invalid nul byte in string: {err}"),
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
                let c_key = CString::new(key).map_err(|err| ConfigError::InvalidNulByte { err })?;
                content.insert(c_key, convert(value)?);
            }
            Ok(ConfigTable { content })
        }
        /// Converts a toml Value.
        fn convert(value: toml::Value) -> Result<ConfigValue, ConfigError> {
            match value {
                toml::Value::String(str) => {
                    let c_str =
                        CString::new(str).map_err(|err| ConfigError::InvalidNulByte { err })?;
                    Ok(ConfigValue::String(c_str))
                }
                toml::Value::Integer(v) => Ok(ConfigValue::Integer(v)),
                toml::Value::Float(v) => Ok(ConfigValue::Float(v)),
                toml::Value::Boolean(v) => Ok(ConfigValue::Boolean(v)),
                toml::Value::Datetime(_) => Err(ConfigError::UnsupportedType {
                    data_type: "datetime",
                })?,
                toml::Value::Array(arr) => {
                    let content: Result<Vec<ConfigValue>, ConfigError> =
                        arr.into_iter().map(|v| convert(v)).collect();
                    Ok(ConfigValue::Array(ConfigArray { content: content? }))
                }
                toml::Value::Table(t) => Ok(ConfigValue::Table(convert_table(t)?)),
            }
        }
        convert_table(table)
    }
    
    pub fn get(&self, key: *const c_char) -> Option<&ConfigValue> {
        let key = unsafe { CStr::from_ptr(key) };
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
                Err(ConfigError::InvalidNulByte { err }) => {
                    assert_eq!(err.nul_position(), nul_pos_in_str)
                }
                _ => panic!("ConfigTable::new should have failed on invalid data"),
            }
        }
    }
}
