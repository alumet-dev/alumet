use std::{
    collections::BTreeMap,
    ffi::{c_char, CStr, CString, NulError},
    ptr, fmt::Display, error::Error,
};

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

pub struct ConfigTable {
    content: BTreeMap<CString, ConfigValue>,
}

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
    
    fn get(&self, key: *const c_char) -> Option<&ConfigValue> {
        let key = unsafe { CStr::from_ptr(key) };
        self.content.get(key)
    }
    
    fn len(&self) -> usize {
        self.content.len()
    }
}

impl ConfigArray {
    fn get(&self, index: usize) -> Option<&ConfigValue> {
        self.content.get(index)
    }
    
    fn len(&self) -> usize {
        self.content.len()
    }
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

#[no_mangle]
pub extern "C" fn config_array_in(table: &ConfigTable, key: *const c_char) -> *const ConfigArray {
    match table.get(key) {
        Some(ConfigValue::Array(a)) => a,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_table_in(table: &ConfigTable, key: *const c_char) -> *const ConfigTable {
    match table.get(key) {
        Some(ConfigValue::Table(t)) => t,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_string_at(array: &ConfigArray, index: usize) -> *const c_char {
    match array.get(index) {
        Some(ConfigValue::String(str)) => str.as_ptr(),
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_int_at(array: &ConfigArray, index: usize) -> *const i64 {
    match array.get(index) {
        Some(ConfigValue::Integer(v)) => v,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_bool_at(array: &ConfigArray, index: usize) -> *const bool {
    match array.get(index) {
        Some(ConfigValue::Boolean(v)) => v,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_float_at(array: &ConfigArray, index: usize) -> *const f64 {
    match array.get(index) {
        Some(ConfigValue::Float(v)) => v,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_array_at(array: &ConfigArray, index: usize) -> *const ConfigArray {
    match array.get(index) {
        Some(ConfigValue::Array(a)) => a,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_table_at(array: &ConfigArray, index: usize) -> *const ConfigTable {
    match array.get(index) {
        Some(ConfigValue::Table(t)) => t,
        _ => ptr::null(),
    }
}

mod tests {
    use std::ffi::{CStr, CString};
    use std::ptr;

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

    #[test]
    fn test_pointers() {
        let table = toml::toml! {
            string = "abc"
            "" = "x and éàü¢»¤€"
            int = 123
            float = 123.456
            true = true
            false = false
            
            array = [0xfafc, 0.42, "test", true, false, [987654321.978465132]]
        };

        let ffi_table = ConfigTable::new(table).unwrap();
        let key_wrong = CString::new("wrong_key").unwrap();
        let key_string = CString::new("string").unwrap();
        let key_string2 = CString::new("").unwrap();
        let key_int = CString::new("int").unwrap();
        let key_float = CString::new("float").unwrap();
        let key_true = CString::new("true").unwrap();
        let key_false = CString::new("false").unwrap();
        let key_array = CString::new("array").unwrap();

        // simple values
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
        
        // array
        let array_ptr_ok = config_array_in(&ffi_table, key_array.as_ptr());
        let array_ptr_wrong = config_array_in(&ffi_table, key_wrong.as_ptr());
        assert_eq!(array_ptr_wrong, ptr::null());
        assert_ne!(array_ptr_ok, ptr::null());

        let array = unsafe { &*array_ptr_ok };
        assert_eq!(array.len(), 6);
        assert_eq!(unsafe{*config_int_at(array, 0)}, 0xfafc);
        assert_eq!(unsafe{*config_float_at(array, 1)}, 0.42);
        assert_eq!(unsafe{CStr::from_ptr(config_string_at(array, 2))}, CString::new("test").unwrap().as_c_str());
        assert_eq!(unsafe{*config_bool_at(array, 3)}, true);
        assert_eq!(unsafe{*config_bool_at(array, 4)}, false);

        assert_ne!(config_array_at(array, 5), ptr::null());
        let sub_array = unsafe {&*config_array_at(array, 5)};
        assert_eq!(sub_array.len(), 1);
        assert_eq!(unsafe{*config_float_at(sub_array, 0)}, 987654321.978465132)
    }
}
