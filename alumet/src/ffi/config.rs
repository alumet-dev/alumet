//! C FFI for the [`crate::config`] module.

use std::{ffi::c_char, ptr};
use crate::config::*;

use super::string::{AStr, NullableAStr};

#[no_mangle]
pub extern "C" fn config_string_in<'a>(table: &'a ConfigTable, key: AStr<'a>) -> NullableAStr<'a> {
    match table.get(key.as_str()) {
        Some(ConfigValue::String(str)) => NullableAStr::from(str),
        _ => NullableAStr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_cstring_in(table: &ConfigTable, key: AStr) -> *const c_char {
    match table.get(key.as_str()) {
        Some(ConfigValue::String(str)) => str.as_ptr() as _,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_int_in(table: &ConfigTable, key: AStr) -> *const i64 {
    match table.get(key.as_str()) {
        Some(ConfigValue::Integer(v)) => v,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_bool_in(table: &ConfigTable, key: AStr) -> *const bool {
    match table.get(key.as_str()) {
        Some(ConfigValue::Boolean(v)) => v,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_float_in(table: &ConfigTable, key: AStr) -> *const f64 {
    match table.get(key.as_str()) {
        Some(ConfigValue::Float(v)) => v,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_array_in(table: &ConfigTable, key: AStr) -> *const ConfigArray {
    match table.get(key.as_str()) {
        Some(ConfigValue::Array(a)) => a,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_table_in(table: &ConfigTable, key: AStr) -> *const ConfigTable {
    match table.get(key.as_str()) {
        Some(ConfigValue::Table(t)) => t,
        _ => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_string_at(array: &mut ConfigArray, index: usize) -> NullableAStr {
    match array.get(index) {
        Some(ConfigValue::String(str)) => NullableAStr::from(str),
        _ => NullableAStr::null(),
    }
}

#[no_mangle]
pub extern "C" fn config_cstring_at(array: &ConfigArray, index: usize) -> *const c_char {
    match array.get(index) {
        Some(ConfigValue::String(str)) => str.as_ptr() as _,
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

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString};
    use super::*;

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
        let key_wrong = AStr::from("wrong_key");
        let key_string = AStr::from("string");
        let key_string2 = AStr::from("");
        let key_int = AStr::from("int");
        let key_float = AStr::from("float");
        let key_true = AStr::from("true");
        let key_false = AStr::from("false");
        let key_array = AStr::from("array");

        // simple values
        let string_ptr_ok = config_string_in(&ffi_table, key_string);
        assert_eq!(
            string_ptr_ok.as_str(),
            Some("abc")
        );
        let string_ptr_wrong = config_cstring_in(&ffi_table, key_wrong);
        assert_eq!(string_ptr_wrong, ptr::null());
        let string_ptr_wrong2 = config_string_in(&ffi_table, key_wrong);
        assert_eq!(string_ptr_wrong2.ptr, ptr::null_mut());

        let string_ptr_ok = config_string_in(&ffi_table, key_string2);
        assert_eq!(
            string_ptr_ok.as_str(),
            Some("x and éàü¢»¤€")
        );

        let int_ptr_ok = config_int_in(&ffi_table, key_int);
        let int_ptr_wrong = config_int_in(&ffi_table, key_wrong);
        assert_eq!(int_ptr_wrong, ptr::null());
        assert_eq!(unsafe { *int_ptr_ok }, 123);

        let float_ptr_ok = config_float_in(&ffi_table, key_float);
        let float_ptr_wrong = config_float_in(&ffi_table, key_wrong);
        assert_eq!(float_ptr_wrong, ptr::null());
        assert_eq!(unsafe { *float_ptr_ok }, 123.456);

        let bool_ptr_true = config_bool_in(&ffi_table, key_true);
        let bool_ptr_false = config_bool_in(&ffi_table, key_false);
        let bool_ptr_wrong = config_bool_in(&ffi_table, key_wrong);
        assert_eq!(bool_ptr_wrong, ptr::null());
        assert_eq!(unsafe { *bool_ptr_true }, true);
        assert_eq!(unsafe { *bool_ptr_false }, false);
        
        // array
        let array_ptr_ok = config_array_in(&ffi_table, key_array);
        let array_ptr_wrong = config_array_in(&ffi_table, key_wrong);
        assert_eq!(array_ptr_wrong, ptr::null());
        assert_ne!(array_ptr_ok, ptr::null());

        let array = unsafe { &*array_ptr_ok };
        assert_eq!(array.len(), 6);
        assert_eq!(unsafe{*config_int_at(array, 0)}, 0xfafc);
        assert_eq!(unsafe{*config_float_at(array, 1)}, 0.42);
        assert_eq!(unsafe{CStr::from_ptr(config_cstring_at(array, 2))}, CString::new("test").unwrap().as_c_str());
        assert_eq!(unsafe{*config_bool_at(array, 3)}, true);
        assert_eq!(unsafe{*config_bool_at(array, 4)}, false);

        assert_ne!(config_array_at(array, 5), ptr::null());
        let sub_array = unsafe {&*config_array_at(array, 5)};
        assert_eq!(sub_array.len(), 1);
        assert_eq!(unsafe{*config_float_at(sub_array, 0)}, 987654321.978465132)
    }
}
