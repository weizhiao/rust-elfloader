use crate::{mmap::Offset, Result};

pub trait ElfObject {
    fn file_name(self) -> CString;
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()>;
    fn transport(&self, offset: usize, len: usize) -> Offset;
}

mod binary {
    use alloc::ffi::CString;

    use crate::{
        mmap::{Offset, OffsetType},
        segment::MASK,
        ElfObject,
    };

    /// An elf file stored in memory
    pub struct ElfBinary<'a> {
        name: &'a str,
        bytes: &'a [u8],
    }

    impl<'bytes> ElfBinary<'bytes> {
        pub const fn new(name: &'bytes str, bytes: &'bytes [u8]) -> Self {
            Self { name, bytes }
        }
    }

    impl<'bytes> ElfObject for ElfBinary<'bytes> {
        fn read(&mut self, buf: &mut [u8], offset: usize) -> crate::Result<()> {
            buf.copy_from_slice(&self.bytes[offset..offset + &buf.len()]);
            Ok(())
        }

        fn transport(&self, offset: usize, len: usize) -> Offset {
            Offset {
                kind: OffsetType::Addr(unsafe { self.bytes.as_ptr().add(offset) }),
                align_offset: offset - (offset & MASK),
                len,
            }
        }

        fn file_name(self) -> CString {
            CString::new(self.name).unwrap()
        }
    }
}

#[cfg(feature = "std")]
mod file {
    use std::{
        ffi::CString,
        fs::File,
        io::{Read, Seek, SeekFrom},
        os::fd::AsRawFd,
    };

    use crate::{
        mmap::{Offset, OffsetType},
        segment::MASK,
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

        fn transport(&self, offset: usize, len: usize) -> Offset {
            Offset {
                align_offset: offset - (offset & MASK),
                len,
                kind: OffsetType::File {
                    fd: self.file.as_raw_fd(),
                    file_offset: offset,
                },
            }
        }

        fn file_name(self) -> CString {
            self.name
        }
    }
}

use alloc::ffi::CString;
pub use binary::ElfBinary;
#[cfg(feature = "std")]
pub use file::ElfFile;
