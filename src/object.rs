//! The original elf object
use crate::{Result, os::RawFile};
use alloc::ffi::CString;
use core::ffi::CStr;

/// The original elf object
pub trait ElfObject {
    /// Returns the elf object name
    fn file_name(&self) -> &CStr;
    /// Read data from the elf object
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()>;
    /// Extracts the raw file descriptor.
    fn as_fd(&self) -> Option<isize>;
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
}

/// An elf file saved in a file
pub struct ElfFile {
    inner: RawFile,
}

impl ElfFile {
    /// # Safety
    ///
    /// The `fd` passed in must be an owned file descriptor; in particular, it must be open.
    pub unsafe fn from_owned_fd(path: &str, raw_fd: i32) -> Self {
        ElfFile {
            inner: RawFile::from_owned_fd(path, raw_fd),
        }
    }

    pub fn from_path(path: impl AsRef<str>) -> Result<Self> {
        Ok(ElfFile {
            inner: RawFile::from_path(path.as_ref())?,
        })
    }
}

impl ElfObject for ElfFile {
    fn file_name(&self) -> &CStr {
        self.inner.file_name()
    }

    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()> {
        self.inner.read(buf, offset)
    }

    fn as_fd(&self) -> Option<isize> {
        self.inner.as_fd()
    }
}
