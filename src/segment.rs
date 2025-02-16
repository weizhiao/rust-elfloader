//! The Memory mapping of elf object
use super::mmap::{self, Mmap, ProtFlags};
use crate::{Result, arch::Phdr};
use core::ffi::c_void;
use core::fmt::Debug;
use core::ptr::NonNull;
use elf::abi::{PF_R, PF_W, PF_X};

#[cfg(target_arch = "aarch64")]
pub const PAGE_SIZE: usize = 0x10000;
#[cfg(not(target_arch = "aarch64"))]
pub const PAGE_SIZE: usize = 0x1000;

pub const MASK: usize = !(PAGE_SIZE - 1);

#[allow(unused)]
pub(crate) struct ELFRelro {
    addr: usize,
    len: usize,
    mprotect: unsafe fn(NonNull<c_void>, usize, ProtFlags) -> Result<()>,
}

impl ELFRelro {
    pub(crate) fn new<M: Mmap>(phdr: &Phdr, base: usize) -> ELFRelro {
        ELFRelro {
            addr: base + phdr.p_vaddr as usize,
            len: phdr.p_memsz as usize,
            mprotect: M::mprotect,
        }
    }
}

/// The Memory mapping of elf object
pub struct ElfSegments {
    pub(crate) memory: NonNull<c_void>,
    /// addr_min
    pub(crate) offset: usize,
    pub(crate) len: usize,
    pub(crate) munmap: unsafe fn(NonNull<c_void>, usize) -> Result<()>,
}

impl Debug for ElfSegments {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ELFSegments")
            .field("memory", &self.memory)
            .field("offset", &self.offset)
            .field("len", &self.len)
            .finish()
    }
}

impl ELFRelro {
    #[inline]
    pub(crate) fn relro(&self) -> Result<()> {
        let end = (self.addr + self.len + PAGE_SIZE - 1) & MASK;
        let start = self.addr & MASK;
        let start_addr = unsafe { NonNull::new_unchecked(start as _) };
        unsafe {
            (self.mprotect)(start_addr, end - start, ProtFlags::PROT_READ)?;
        }
        Ok(())
    }
}

impl Drop for ElfSegments {
    fn drop(&mut self) {
        unsafe {
            (self.munmap)(self.memory, self.len).unwrap();
        }
    }
}

impl ElfSegments {
    pub fn new(
        memory: NonNull<c_void>,
        len: usize,
        munmap: unsafe fn(NonNull<c_void>, usize) -> Result<()>,
    ) -> Self {
        ElfSegments {
            memory,
            offset: 0,
            len,
            munmap,
        }
    }

    #[inline]
    pub(crate) fn map_prot(prot: u32) -> mmap::ProtFlags {
        mmap::ProtFlags::from_bits_retain(
            ((prot & PF_X) << 2 | prot & PF_W | (prot & PF_R) >> 2) as _,
        )
    }

    #[inline]
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// base = memory_addr - offset
    #[inline]
    pub fn base(&self) -> usize {
        unsafe { self.memory.as_ptr().cast::<u8>().sub(self.offset) as usize }
    }

    /// start = memory_addr - offset
    #[inline]
    pub(crate) fn as_mut_slice(&self) -> &'static mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.memory.as_ptr().cast::<u8>().sub(self.offset),
                self.len,
            )
        }
    }
}
