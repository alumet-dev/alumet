use std::{ffi::c_char, ptr};
use crate::config::*;

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
