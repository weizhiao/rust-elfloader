use super::HostError;
use crate::Result;
use alloc::string::ToString;
use core::{
    ffi::{CStr, c_void},
    ptr::NonNull,
};

pub(super) struct NativeLibrary(NonNull<c_void>);

// The native loader synchronizes access to its handles. The last owner closes it.
unsafe impl Send for NativeLibrary {}
unsafe impl Sync for NativeLibrary {}

impl NativeLibrary {
    pub(super) unsafe fn open(path: &CStr) -> Result<Self> {
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        NonNull::new(handle)
            .map(Self)
            .ok_or_else(|| last_error("dlopen"))
    }

    pub(super) fn handle(&self) -> *mut c_void {
        self.0.as_ptr()
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
