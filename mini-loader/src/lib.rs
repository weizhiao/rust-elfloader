#![no_std]
extern crate alloc;

pub mod syscall;

use core::{
    ffi::{c_int, CStr},
    ptr::NonNull,
};
use elf_loader::{mmap::Mmap, object::ElfObject};
use syscall::{lseek, mmap, mprotect, munmap, open, read, SEEK_SET};

pub struct MyFile {
    fd: i32,
}

impl ElfObject for MyFile {
    fn file_name(&self) -> &CStr {
        c""
    }

    fn read(&mut self, buf: &mut [u8], offset: usize) -> elf_loader::Result<()> {
        lseek(self.fd, offset as isize, SEEK_SET);
        read(self.fd, buf.as_mut_ptr().cast(), buf.len());
        Ok(())
    }

    fn as_fd(&self) -> Option<i32> {
        Some(self.fd)
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
        offset: usize,
        fd: Option<i32>,
        need_copy: &mut bool,
    ) -> elf_loader::Result<core::ptr::NonNull<core::ffi::c_void>> {
        *need_copy = false;
        let ptr = mmap(
            addr.unwrap_or(0) as _,
            len,
            prot.bits(),
            flags.bits(),
            fd.unwrap(),
            offset as _,
        );
        Ok(NonNull::new_unchecked(ptr))
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: elf_loader::mmap::ProtFlags,
        flags: elf_loader::mmap::MapFlags,
    ) -> elf_loader::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let ptr = mmap(addr as _, len, prot.bits(), flags.bits(), -1, 0);
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
