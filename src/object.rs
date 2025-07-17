//! The original elf object
use crate::{Result, os::from_path};
use alloc::ffi::CString;
use core::ffi::CStr;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::HANDLE;

/// The original elf object
pub trait ElfObject {
    /// Returns the elf object name
    fn file_name(&self) -> &CStr;
    /// Read data from the elf object
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()>;
    /// Extracts the raw file descriptor.
    fn as_fd(&self) -> Option<isize>;
    #[cfg(target_os = "windows")]
    /// Returns the file mapping handle for the elf object.
    fn as_mapping_handle(&self) -> Option<isize>;
}

/// The original elf object
pub trait ElfObjectAsync: ElfObject {
    /// Read data from the elf object
    fn read_async(
        &mut self,
        buf: &mut [u8],
        offset: usize,
    ) -> impl core::future::Future<Output = Result<()>> + Send;
}

/// An elf file stored in memory
pub struct ElfBinary<'bytes> {
    name: CString,
    bytes: &'bytes [u8],
}

impl<'bytes> ElfBinary<'bytes> {
    pub fn new(name: &str, bytes: &'bytes [u8]) -> Self {
        Self {
            name: CString::new(name).unwrap(),
            bytes,
        }
    }
}

impl<'bytes> ElfObject for ElfBinary<'bytes> {
    fn read(&mut self, buf: &mut [u8], offset: usize) -> crate::Result<()> {
        buf.copy_from_slice(&self.bytes[offset..offset + buf.len()]);
        Ok(())
    }

    fn file_name(&self) -> &CStr {
        &self.name
    }

    fn as_fd(&self) -> Option<isize> {
        None
    }

    #[cfg(target_os = "windows")]
    fn as_mapping_handle(&self) -> Option<isize> {
        None
    }
}

/// An elf file saved in a file
pub struct ElfFile {
    #[allow(unused)]
    pub(crate) name: CString,
    #[allow(unused)]
    pub(crate) fd: isize,
    #[cfg(target_os = "windows")]
    /// Stores the mapping handle for the file.
    #[allow(unused)]
    pub(crate) mapping: HANDLE,
}

impl ElfFile {
    #[cfg(not(target_os = "windows"))]
    /// # Safety
    ///
    /// The `fd` passed in must be an owned file descriptor; in particular, it must be open.
    pub unsafe fn from_owned_fd(path: &str, raw_fd: i32) -> Self {
        ElfFile {
            name: CString::new(path).unwrap(),
            fd: raw_fd as isize,
        }
    }

    #[cfg(target_os = "windows")]
    /// # Safety
    ///
    /// The `file_handle` passed in must be an owned file handle; in particular, it must be open.
    /// The `mapping_handle` must be a valid memory mapping handle for the file.
    /// It will pass the ownership of the file handle and mapping handle to the `ElfFile`.
    /// The file and file handle will be closed when the `ElfFile` is dropped.
    pub unsafe fn from_owned_fd(path: &str, file_handle: isize, mapping_handle: HANDLE) -> Self {
        ElfFile {
            name: CString::new(path).unwrap(),
            fd: file_handle as isize,
            mapping: mapping_handle,
        }
    }

    pub fn from_path(path: &str) -> Result<Self> {
        from_path(path)
    }
}
