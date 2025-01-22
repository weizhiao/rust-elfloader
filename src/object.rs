//! The original elf object
use crate::{mmap::MmapOffset, Result};
use core::ffi::CStr;

/// The original elf object
pub trait ElfObject {
    /// Returns the elf object name
    fn file_name(&self) -> &CStr;
    /// Read data from the elf object
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()>;
    /// Transport the offset to the mapped memory. The `offset` argument is always aligned to the page size
    fn transport(&self, offset: usize, len: usize) -> MmapOffset;
}

mod binary {
    use alloc::ffi::CString;
    use core::ffi::CStr;

    use crate::{
        mmap::{MmapOffset, OffsetType},
        ElfObject,
    };

    /// An elf file stored in memory
    pub struct ElfBinary<'a> {
        name: CString,
        bytes: &'a [u8],
    }

    impl<'bytes> ElfBinary<'bytes> {
        pub fn new(name: &'bytes str, bytes: &'bytes [u8]) -> Self {
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

        fn transport(&self, offset: usize, len: usize) -> MmapOffset {
            MmapOffset {
                kind: OffsetType::Addr(unsafe { self.bytes.as_ptr().add(offset) }),
                len,
            }
        }

        fn file_name(&self) -> &CStr {
            &self.name
        }
    }
}

#[cfg(feature = "std")]
mod file {
    use core::ffi::CStr;
    use std::{
        ffi::CString,
        fs::File,
        io::{Read, Seek, SeekFrom},
        os::fd::AsRawFd,
    };

    use crate::{
        mmap::{MmapOffset, OffsetType},
        ElfObject, Result,
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

        fn transport(&self, offset: usize, len: usize) -> MmapOffset {
            MmapOffset {
                len,
                kind: OffsetType::File {
                    fd: self.file.as_raw_fd(),
                    file_offset: offset,
                },
            }
        }

        fn file_name(&self) -> &CStr {
            &self.name
        }
    }
}

pub use binary::ElfBinary;
#[cfg(feature = "std")]
pub use file::ElfFile;
