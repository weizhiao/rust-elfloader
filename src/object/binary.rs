use crate::ElfObject;
use alloc::ffi::CString;
use core::ffi::CStr;

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
        buf.copy_from_slice(&self.bytes[offset..offset + &buf.len()]);
        Ok(())
    }

    fn file_name(&self) -> &CStr {
        &self.name
    }

    fn as_fd(&self) -> Option<i32> {
        None
    }
}
