#![no_std]
extern crate alloc;

use alloc::ffi::CString;
use core::{
    ffi::{c_int, CStr},
    ptr::NonNull,
};
use elf_loader::{
    mmap::{Mmap, Offset, OffsetType},
    object::ElfObject,
    segment::PAGE_SIZE,
    ThreadLocal, Unwind,
};
use syscall::{lseek, mmap, mprotect, munmap, open, read, MAP_ANONYMOUS, SEEK_SET};
pub mod syscall;

pub struct MyFile {
    fd: i32,
}

impl ElfObject for MyFile {
    fn file_name(self) -> alloc::ffi::CString {
        CString::new("").unwrap()
    }

    fn read(&mut self, buf: &mut [u8], offset: usize) -> elf_loader::Result<()> {
        lseek(self.fd, offset as isize, SEEK_SET);
        read(self.fd, buf.as_mut_ptr().cast(), buf.len());
        Ok(())
    }

    fn transport(&self, offset: usize, len: usize) -> elf_loader::mmap::Offset {
        Offset {
            align_offset: offset - (offset & !(PAGE_SIZE - 1)),
            len,
            kind: OffsetType::File {
                fd: self.fd,
                file_offset: offset,
            },
        }
    }
}

impl MyFile {
    pub fn new(path: &CStr) -> Self {
        const O_RDONLY: c_int = 0;
        let fd = open(path.as_ptr(), O_RDONLY, 0);
        Self { fd }
    }
}

pub struct MmapImpl;

impl Mmap for MmapImpl {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: elf_loader::mmap::ProtFlags,
        flags: elf_loader::mmap::MapFlags,
        offset: elf_loader::mmap::Offset,
    ) -> elf_loader::Result<core::ptr::NonNull<core::ffi::c_void>> {
        match offset.kind {
            elf_loader::mmap::OffsetType::File { fd, file_offset } => {
                let ptr = mmap(
                    addr.unwrap_or(0) as _,
                    len,
                    prot.bits(),
                    flags.bits(),
                    fd,
                    // offset是当前段在文件中的偏移，需要按照页对齐，否则mmap会失败
                    (file_offset & !(PAGE_SIZE - 1)) as _,
                );
                Ok(NonNull::new_unchecked(ptr))
            }
            elf_loader::mmap::OffsetType::Addr(_) => todo!(),
        }
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: elf_loader::mmap::ProtFlags,
        flags: elf_loader::mmap::MapFlags,
    ) -> elf_loader::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let ptr = mmap(
            addr as _,
            len,
            prot.bits(),
            flags.bits() | MAP_ANONYMOUS,
            -1,
            0,
        );
        Ok(NonNull::new_unchecked(ptr))
    }

    unsafe fn munmap(
        addr: core::ptr::NonNull<core::ffi::c_void>,
        len: usize,
    ) -> elf_loader::Result<()> {
        let _ = munmap(addr.as_ptr(), len);
        Ok(())
    }

    unsafe fn mprotect(
        addr: core::ptr::NonNull<core::ffi::c_void>,
        len: usize,
        prot: elf_loader::mmap::ProtFlags,
    ) -> elf_loader::Result<()> {
        let _ = mprotect(addr.as_ptr(), len, prot.bits());
        Ok(())
    }
}

pub struct MyThreadLocal;

impl ThreadLocal for MyThreadLocal {
    unsafe fn new(_phdr: &elf_loader::arch::Phdr, _base: usize) -> Option<Self> {
        None
    }

    unsafe fn module_id(&self) -> usize {
        0
    }
}

pub struct MyUnwind;

impl Unwind for MyUnwind {
    unsafe fn new(
        _phdr: &elf_loader::arch::Phdr,
        _map_range: core::ops::Range<usize>,
    ) -> Option<Self> {
        None
    }
}
