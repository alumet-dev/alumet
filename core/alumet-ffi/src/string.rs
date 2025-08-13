use std::borrow::Cow;
use std::ffi::{CStr, c_char};
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ptr::NonNull;

/// FFI equivalent to [`String`].
///
/// When modifying an AString, you must ensure that it remains valid UTF-8.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct AString {
    pub(super) len: usize,
    pub(super) capacity: usize,
    pub(super) ptr: NonNull<c_char>,
}

/// FFI equivalent to [`&str`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AStr<'a> {
    pub(super) len: usize,
    pub(super) ptr: NonNull<c_char>,
    _marker: &'a PhantomData<()>,
}

/// FFI equivalent to [`Option<&str>`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NullableAStr<'a> {
    pub(super) len: usize,
    pub(super) ptr: *const c_char, // nullable but const
    _marker: &'a PhantomData<()>,
}

impl Drop for AString {
    fn drop(&mut self) {
        let data = unsafe { String::from_raw_parts(self.ptr.as_ptr() as _, self.len, self.capacity) };
        drop(data);
    }
}

impl AString {
    pub fn as_str(&self) -> &str {
        self.into() // see below
    }
}

impl ToString for AString {
    fn to_string(&self) -> String {
        String::from(self) // see below
    }
}

impl<'a> AStr<'a> {
    pub fn as_str(&self) -> &str {
        self.into()
    }
}

impl<'a> ToString for AStr<'a> {
    fn to_string(&self) -> String {
        String::from(self.as_str())
    }
}

impl<'a> NullableAStr<'a> {
    pub fn null() -> Self {
        Self {
            len: 0,
            ptr: std::ptr::null_mut(),
            _marker: &PhantomData,
        }
    }

    #[allow(dead_code)]
    pub fn new(s: AStr) -> Self {
        Self {
            len: s.len,
            ptr: s.ptr.as_ptr(),
            _marker: &PhantomData,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        if self.ptr.is_null() {
            None
        } else {
            let s = unsafe {
                let slice = std::slice::from_raw_parts(self.ptr as _, self.len);
                std::str::from_utf8_unchecked(slice)
            };
            Some(s)
        }
    }
}

/// Creates a new `AString` from a C string `chars`, which must be null-terminated.
///
/// The returned `AString` is a copy of the C string.
/// To free the `AString`, use [`astring_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn astring(chars: *const c_char) -> AString {
    let string = unsafe { CStr::from_ptr(chars) }
        .to_str()
        .expect("chars should be a valid UTF-8 string for astring(chars)")
        .to_string();
    let mut string = ManuallyDrop::new(string);
    let ptr = unsafe { NonNull::new_unchecked(string.as_mut_ptr() as _) };
    let len = string.len();
    AString {
        len,
        capacity: len,
        ptr,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astr_copy(astr: AStr) -> AString {
    AString::from(astr.as_str())
}

#[unsafe(no_mangle)]
pub extern "C" fn astr_copy_nonnull(astr: NullableAStr) -> AString {
    AString::from(astr.as_str().expect("astr should be non-null for astr_copy_nonnull()"))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astr<'a>(chars: *const c_char) -> AStr<'a> {
    let cstring = unsafe { CStr::from_ptr(chars) };
    let str = cstring
        .to_str()
        .expect("chars should be a valid UTF-8 string for astring(chars)");
    AStr {
        len: str.len(),
        ptr: unsafe { NonNull::new_unchecked(cstring.as_ptr() as _) },
        _marker: &PhantomData,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astring_ref<'a>(string: AString) -> AStr<'a> {
    let string = ManuallyDrop::new(string);
    AStr {
        len: string.len,
        ptr: string.ptr,
        _marker: &PhantomData,
    }
}

/// Frees a `AString`.
#[unsafe(no_mangle)]
pub extern "C" fn astring_free(string: AString) {
    drop(string); // see impl Drop for AString
}

// ====== Comparisons ======
impl PartialEq for AString {
    fn eq(&self, other: &Self) -> bool {
        let a: &str = self.into();
        let b: &str = other.into();
        a == b
    }
}

// ====== Conversions *to* AStr, AString ======
impl From<&str> for AString {
    fn from(value: &str) -> Self {
        let copy = value.to_string();
        let mut data = ManuallyDrop::new(copy);
        let ptr = unsafe { NonNull::new_unchecked(data.as_mut_ptr() as _) };
        AString {
            len: data.len(),
            capacity: data.capacity(),
            ptr,
        }
    }
}

impl From<String> for AString {
    fn from(value: String) -> Self {
        let mut data = ManuallyDrop::new(value); // don't drop the String, transfer the ownership to AString
        let ptr = unsafe { NonNull::new_unchecked(data.as_mut_ptr() as _) };
        AString {
            len: data.len(),
            capacity: data.capacity(),
            ptr,
        }
    }
}

impl From<&String> for AString {
    fn from(value: &String) -> Self {
        AString::from(value.to_owned())
    }
}

impl<'a> From<&'a AString> for AStr<'a> {
    fn from(value: &'a AString) -> AStr<'a> {
        AStr {
            len: value.len,
            ptr: value.ptr,
            _marker: &PhantomData,
        }
    }
}

impl<'a> From<&'a mut str> for AStr<'a> {
    fn from(value: &'a mut str) -> Self {
        AStr {
            len: value.len(),
            ptr: unsafe { NonNull::new_unchecked(value.as_mut_ptr() as _) },
            _marker: &PhantomData,
        }
    }
}

impl<'a> From<&'a mut String> for AStr<'a> {
    fn from(value: &'a mut String) -> Self {
        AStr::from(value.as_mut_str())
    }
}

impl<'a> From<&'a str> for AStr<'a> {
    fn from(value: &'a str) -> Self {
        AStr {
            len: value.len(),
            ptr: unsafe { NonNull::new_unchecked(value.as_ptr() as _) },
            _marker: &PhantomData,
        }
    }
}

impl<'a> From<&'a str> for NullableAStr<'a> {
    fn from(value: &'a str) -> Self {
        NullableAStr {
            len: value.len(),
            ptr: value.as_ptr() as _,
            _marker: &PhantomData,
        }
    }
}

impl<'a> From<&'a String> for NullableAStr<'a> {
    fn from(value: &'a String) -> Self {
        NullableAStr::from(value.as_str())
    }
}

// ====== Conversions *from* AStr, AString ======
impl<'a> From<&'a AString> for &'a str {
    /// Converts a `&AString` into a `&str`.
    /// The returned string slice has the same lifetime as the converted value,
    /// because it simply points to the memory owned by the `AString`.
    fn from(value: &'a AString) -> &'a str {
        unsafe {
            let slice = std::slice::from_raw_parts(value.ptr.as_ptr() as _, value.len);
            std::str::from_utf8_unchecked(slice) // TODO should we check?
        }
    }
}

impl From<AString> for String {
    /// Turns an `AString` into a `String`, consuming the `AString`.
    /// The memory will be deallocated when the `String` is dropped.
    fn from(value: AString) -> String {
        unsafe { String::from_raw_parts(value.ptr.as_ptr() as _, value.len, value.capacity) }
    }
}

impl From<&AString> for String {
    /// Copies this `AString` to a `String`.
    fn from(value: &AString) -> String {
        let slice: &str = value.into();
        String::from(slice)
    }
}

impl<'a> From<&'a AString> for Cow<'a, str> {
    fn from(value: &'a AString) -> Cow<'a, str> {
        let slice: &str = value.into();
        Cow::from(slice)
    }
}

impl<'a> From<&'a AStr<'a>> for &'a str {
    /// Converts a `&AString` into a `&str`.
    /// The returned string slice has the same lifetime as the converted value,
    /// because it simply points to the memory owned by the `AString`.
    fn from(value: &'a AStr) -> &'a str {
        unsafe {
            let slice = std::slice::from_raw_parts(value.ptr.as_ptr() as _, value.len);
            std::str::from_utf8_unchecked(slice) // TODO should we check?
        }
    }
}

impl<'a> From<AStr<'a>> for String {
    fn from(value: AStr<'a>) -> Self {
        value.as_str().to_string()
    }
}
