use crate::Error;
use crate::mmap::{MapFlags, Mmap, ProtFlags};
use alloc::string::ToString;
use core::ptr::NonNull;
use libc::{mmap, mprotect, munmap};

/// An implementation of Mmap trait
pub struct MmapImpl;

impl Mmap for MmapImpl {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
        offset: usize,
        fd: Option<i32>,
        need_copy: &mut bool,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let ptr = if let Some(fd) = fd {
            unsafe {
                mmap(
                    addr.unwrap_or(0) as _,
                    len,
                    prot.bits(),
                    flags.bits(),
                    fd,
                    offset as _,
                )
            }
        } else {
            *need_copy = true;
            if let Some(addr) = addr {
                addr as _
            } else {
                unsafe {
                    mmap(
                        addr.unwrap_or(0) as _,
                        len,
                        ProtFlags::PROT_WRITE.bits(),
                        (flags | MapFlags::MAP_ANONYMOUS).bits(),
                        -1,
                        0,
                    )
                }
            }
        };
        if core::ptr::eq(ptr, libc::MAP_FAILED) {
            return Err(map_error("mmap failed"));
        }
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let ptr = unsafe {
            mmap(
                addr as _,
                len,
                prot.bits(),
                flags.union(MapFlags::MAP_ANONYMOUS).bits(),
                -1,
                0,
            )
        };
        if core::ptr::eq(ptr, libc::MAP_FAILED) {
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
        prot: ProtFlags,
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
