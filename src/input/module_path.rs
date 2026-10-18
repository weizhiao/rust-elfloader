use super::{Path, PathBuf, PathStr};
use crate::sync::Arc;
use alloc::ffi::{CString, NulError};
use core::{ffi::CStr, fmt, ops::Deref};

/// An immutable loaded path or source identifier with shared C-string storage.
///
/// Construction preserves the path bytes and rejects embedded NULs. Clones
/// retain the same allocation, whose address remains stable until the last
/// owner is dropped. Path views exclude the terminating NUL.
#[derive(Clone)]
pub struct ModulePath {
    storage: Arc<CStr>,
}

impl ModulePath {
    /// Creates an empty path with shared C-string storage.
    pub fn empty() -> Self {
        Self::from_validated(CString::default())
    }

    fn from_validated(path: CString) -> Self {
        Self {
            storage: Arc::from(path.into_boxed_c_str()),
        }
    }

    /// Borrows the path without its terminating NUL.
    pub fn as_path(&self) -> &Path {
        // SAFETY: construction either preserves a Path's platform encoding,
        // or accepts arbitrary C-string bytes only on Unix/bare-metal targets.
        let path = unsafe { PathStr::new(self.as_c_str().to_bytes()) };
        Path::new(path)
    }

    /// Borrows the same storage as a NUL-terminated C string.
    pub fn as_c_str(&self) -> &CStr {
        &self.storage
    }
}

impl Default for ModulePath {
    fn default() -> Self {
        Self::empty()
    }
}

impl TryFrom<PathBuf> for ModulePath {
    type Error = NulError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        CString::new(path.into_bytes()).map(Self::from_validated)
    }
}

impl TryFrom<&Path> for ModulePath {
    type Error = NulError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        CString::new(path.as_bytes()).map(Self::from_validated)
    }
}

// Arbitrary C strings are valid path bytes here; Windows paths instead carry
// the platform encoding through Path/PathBuf conversions above.
#[cfg(any(unix, target_os = "none"))]
impl From<CString> for ModulePath {
    fn from(path: CString) -> Self {
        Self::from_validated(path)
    }
}

impl Deref for ModulePath {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<Path> for ModulePath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl fmt::Debug for ModulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_path(), f)
    }
}

impl fmt::Display for ModulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_path(), f)
    }
}
