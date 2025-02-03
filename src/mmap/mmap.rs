use super::{MapFlags, Mmap, ProtFlags};
use crate::Error;
use core::ptr::NonNull;
use libc::{mmap, mprotect, munmap};

/// An implementation of Mmap trait
pub struct MmapImpl;

impl Mmap for MmapImpl {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: super::ProtFlags,
        flags: super::MapFlags,
        offset: usize,
        fd: Option<i32>,
        need_copy: &mut bool,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let (flags, prot, fd) = if let Some(fd) = fd {
            (flags, prot, fd)
        } else {
            *need_copy = true;
            (flags | MapFlags::MAP_ANONYMOUS, ProtFlags::PROT_WRITE, -1)
        };
        let ptr = unsafe {
            mmap(
                addr.unwrap_or(0) as _,
                len,
                prot.bits(),
                flags.bits(),
                fd,
                offset as _,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(map_error("mmap failed"));
        }
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: super::ProtFlags,
        flags: super::MapFlags,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let ptr = unsafe { mmap(addr as _, len, prot.bits(), flags.bits(), -1, 0) };
        if ptr == libc::MAP_FAILED {
            return Err(map_error("mmap anonymous failed"));
        }
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }

    unsafe fn munmap(addr: core::ptr::NonNull<core::ffi::c_void>, len: usize) -> crate::Result<()> {
        let res = unsafe { munmap(addr.as_ptr(), len) };
        if res != 0 {
            return Err(map_error("munmap failed"));
        }
        Ok(())
    }

    unsafe fn mprotect(
        addr: core::ptr::NonNull<core::ffi::c_void>,
        len: usize,
        prot: super::ProtFlags,
    ) -> crate::Result<()> {
        let res = unsafe { mprotect(addr.as_ptr(), len, prot.bits()) };
        if res != 0 {
            return Err(map_error("mprotect failed"));
        }
        Ok(())
    }
}

#[cold]
#[inline(never)]
fn map_error(msg: &str) -> Error {
    Error::MmapError {
        msg: msg.to_string(),
    }
}
