use std::{
    collections::BTreeMap,
    ffi::{c_char, CStr, CString, NulError},
    ptr,
};

pub struct ConfigTable {
    content: BTreeMap<CString, ConfigValue>,
}

impl ConfigTable {
    fn get(&self, key: *const c_char) -> Option<&ConfigValue> {
        let key = unsafe { CStr::from_ptr(key) };
        self.content.get(key)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    UnsupportedType { data_type: &'static str },
    InvalidNulByte { err: NulError },
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
}

pub struct ConfigArray {
    content: Vec<ConfigValue>,
}

pub enum ConfigValue {
    String(CString),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    // Datetime(ConfigDatetime), not supported yet
    Array(ConfigArray),
    Table(ConfigTable),
}

pub struct ConfigString {
    content: String,
}

#[no_mangle]
pub extern "C" fn config_string_in(table: &ConfigTable, key: *const c_char) -> *const c_char {
    match table.get(key) {
        Some(ConfigValue::String(str)) => str.as_ptr(),
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_int_in(table: &ConfigTable, key: *const c_char) -> *const i64 {
    match table.get(key) {
        Some(ConfigValue::Integer(v)) => v,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_bool_in(table: &ConfigTable, key: *const c_char) -> *const bool {
    match table.get(key) {
        Some(ConfigValue::Boolean(v)) => v,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_float_in(table: &ConfigTable, key: *const c_char) -> *const f64 {
    match table.get(key) {
        Some(ConfigValue::Float(v)) => v,
        _ => ptr::null(),
    }
}

mod tests {
    use crate::config::{config_bool_in, config_float_in, config_int_in, config_string_in};
    use std::ffi::{CStr, CString};
    use std::ptr;

    use super::{ConfigError, ConfigTable};

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

    #[test]
    fn test_pointers() {
        let table = toml::toml! {
            string = "abc"
            "" = "x and éàü¢»¤€"
            int = 123
            float = 123.456
            true = true
            false = false
        };

        let ffi_table = ConfigTable::new(table).unwrap();
        let key_wrong = CString::new("wrong_key").unwrap();
        let key_string = CString::new("string").unwrap();
        let key_string2 = CString::new("").unwrap();
        let key_int = CString::new("int").unwrap();
        let key_float = CString::new("float").unwrap();
        let key_true = CString::new("true").unwrap();
        let key_false = CString::new("false").unwrap();

        let string_ptr_ok = config_string_in(&ffi_table, key_string.as_ptr());
        let string_ptr_wrong = config_string_in(&ffi_table, key_wrong.as_ptr());
        assert_eq!(string_ptr_wrong, ptr::null());
        assert_eq!(
            unsafe { CStr::from_ptr(string_ptr_ok) },
            CString::new("abc").unwrap().as_c_str()
        );

        let string_ptr_ok = config_string_in(&ffi_table, key_string2.as_ptr());
        assert_eq!(
            unsafe { CStr::from_ptr(string_ptr_ok) },
            CString::new("x and éàü¢»¤€").unwrap().as_c_str()
        );

        let int_ptr_ok = config_int_in(&ffi_table, key_int.as_ptr());
        let int_ptr_wrong = config_int_in(&ffi_table, key_wrong.as_ptr());
        assert_eq!(int_ptr_wrong, ptr::null());
        assert_eq!(unsafe { *int_ptr_ok }, 123);

        let float_ptr_ok = config_float_in(&ffi_table, key_float.as_ptr());
        let float_ptr_wrong = config_float_in(&ffi_table, key_wrong.as_ptr());
        assert_eq!(float_ptr_wrong, ptr::null());
        assert_eq!(unsafe { *float_ptr_ok }, 123.456);

        let bool_ptr_true = config_bool_in(&ffi_table, key_true.as_ptr());
        let bool_ptr_false = config_bool_in(&ffi_table, key_false.as_ptr());
        let bool_ptr_wrong = config_bool_in(&ffi_table, key_wrong.as_ptr());
        assert_eq!(bool_ptr_wrong, ptr::null());
        assert_eq!(unsafe { *bool_ptr_true }, true);
        assert_eq!(unsafe { *bool_ptr_false }, false);
    }
}
