use super::HostError;
use crate::{IoError, Result, input::Path};
use alloc::{ffi::CString, string::ToString};
use core::{
    ffi::{CStr, c_void},
    ptr::NonNull,
};

pub(super) struct NativeLibrary {
    handle: NonNull<c_void>,
}

// The native loader synchronizes access to its handles. The last owner closes it.
unsafe impl Send for NativeLibrary {}
unsafe impl Sync for NativeLibrary {}

impl NativeLibrary {
    pub(super) unsafe fn open(path: &Path) -> Result<Self> {
        // A slash keeps dlopen from treating a relative filename as a SONAME.
        let path = if path.is_absolute() {
            path.into()
        } else {
            Path::new(".").join(path)
        };
        let path = CString::new(path.as_bytes()).map_err(|_| IoError::NullByteInPath)?;
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        NonNull::new(handle)
            .map(|handle| Self { handle })
            .ok_or_else(|| last_error("dlopen"))
    }

    pub(super) fn handle(&self) -> *mut c_void {
        self.handle.as_ptr()
    }

    pub(super) fn lookup(&self, name: &CStr, version: Option<&CStr>) -> Result<*mut c_void> {
        unsafe {
            libc::dlerror();
            let address = match version {
                Some(version) => libc::dlvsym(self.handle(), name.as_ptr(), version.as_ptr()),
                None => libc::dlsym(self.handle(), name.as_ptr()),
            };
            let error = libc::dlerror();
            if error.is_null() {
                // A null-valued symbol is distinct from a lookup failure.
                Ok(address)
            } else {
                Err(HostError::Native {
                    operation: if version.is_some() { "dlvsym" } else { "dlsym" },
                    message: alloc::format!(
                        "{}: {}",
                        name.to_string_lossy(),
                        CStr::from_ptr(error).to_string_lossy()
                    ),
                }
                .into())
            }
        }
    }
}

impl Drop for NativeLibrary {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.handle());
        }
    }
}

pub(super) fn last_error(operation: &'static str) -> crate::Error {
    let error = unsafe { libc::dlerror() };
    let message = if error.is_null() {
        "native loader returned no diagnostic".to_string()
    } else {
        unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned()
    };
    HostError::Native { operation, message }.into()
}
