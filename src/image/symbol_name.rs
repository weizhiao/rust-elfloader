use crate::sync::Arc;
use alloc::{
    ffi::{CString, NulError},
    string::String,
    vec::Vec,
};
use core::{
    borrow::Borrow,
    ffi::CStr,
    hash::{Hash, Hasher},
};

/// A shared, NUL-terminated symbol name; UTF-8 is not required.
///
/// Use `From` for C strings or `TryFrom` for unterminated strings and bytes.
/// `TryFrom` rejects interior NULs and adds the terminator. Byte views exclude
/// the terminator; C-string views include it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SymbolName(Arc<CStr>);

impl SymbolName {
    /// Returns the symbol name as a NUL-terminated C string.
    #[inline]
    pub fn as_c_str(&self) -> &CStr {
        &self.0
    }

    /// Returns the symbol name bytes without the terminating NUL.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.as_c_str().to_bytes()
    }
}

impl From<CString> for SymbolName {
    fn from(name: CString) -> Self {
        // From<Box<T>> also supports platforms using portable-atomic-util.
        Self(Arc::from(name.into_boxed_c_str()))
    }
}

impl From<&CStr> for SymbolName {
    fn from(name: &CStr) -> Self {
        Self::from(CString::from(name))
    }
}

impl From<&SymbolName> for SymbolName {
    fn from(name: &SymbolName) -> Self {
        name.clone()
    }
}

impl TryFrom<&str> for SymbolName {
    type Error = NulError;
    fn try_from(name: &str) -> Result<Self, Self::Error> {
        CString::new(name).map(Self::from)
    }
}

impl TryFrom<String> for SymbolName {
    type Error = NulError;
    fn try_from(name: String) -> Result<Self, Self::Error> {
        CString::new(name).map(Self::from)
    }
}

impl TryFrom<&[u8]> for SymbolName {
    type Error = NulError;
    fn try_from(name: &[u8]) -> Result<Self, Self::Error> {
        CString::new(name).map(Self::from)
    }
}

impl TryFrom<Vec<u8>> for SymbolName {
    type Error = NulError;
    fn try_from(name: Vec<u8>) -> Result<Self, Self::Error> {
        CString::new(name).map(Self::from)
    }
}

impl AsRef<CStr> for SymbolName {
    fn as_ref(&self) -> &CStr {
        self.as_c_str()
    }
}

impl AsRef<[u8]> for SymbolName {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Borrow<[u8]> for SymbolName {
    fn borrow(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Hash for SymbolName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Match Borrow<[u8]> so borrowed hash lookups use the same hash.
        self.as_bytes().hash(state);
    }
}
