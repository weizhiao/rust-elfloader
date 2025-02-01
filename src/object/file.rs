use crate::{ElfObject, Result};
use core::ffi::CStr;
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::fd::AsRawFd,
};

/// An elf file saved in a file
pub struct ElfFile {
    name: CString,
    file: File,
}

impl ElfFile {
    pub fn new(name: &str, file: File) -> Self {
        ElfFile {
            name: CString::new(name).unwrap(),
            file,
        }
    }
}

impl ElfObject for ElfFile {
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()> {
        self.file.seek(SeekFrom::Start(offset as _))?;
        self.file.read_exact(buf)?;
        Ok(())
    }

    fn file_name(&self) -> &CStr {
        &self.name
    }

    fn as_fd(&self) -> Option<i32> {
        Some(self.file.as_raw_fd())
    }
}
