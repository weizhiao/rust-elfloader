use super::{MapFlags, Mmap, ProtFlags};
use crate::segment::MASK;
use crate::Error;
use core::ptr::NonNull;
use core::slice::{from_raw_parts, from_raw_parts_mut};
use libc::{mmap, mprotect, munmap};

pub struct MmapImpl;

impl Mmap for MmapImpl {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: super::ProtFlags,
        flags: super::MapFlags,
        offset: super::Offset,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        match offset.kind {
            super::OffsetType::File { fd, file_offset } => {
                let ptr = mmap(
                    addr.unwrap_or(0) as _,
                    len,
                    prot.bits(),
                    flags.bits(),
                    fd,
                    // offset是当前段在文件中的偏移，需要按照页对齐，否则mmap会失败
                    (file_offset & MASK) as _,
                );
                if ptr == libc::MAP_FAILED {
                    return Err(map_error("mmap failed"));
                }
                Ok(NonNull::new_unchecked(ptr))
            }
            super::OffsetType::Addr(data_ptr) => {
                let ptr = mmap(
                    addr.unwrap_or(0) as _,
                    len,
                    ProtFlags::PROT_WRITE.bits(),
                    (flags | MapFlags::MAP_ANONYMOUS).bits(),
                    -1,
                    0,
                );
                if ptr == libc::MAP_FAILED {
                    return Err(map_error("mmap failed"));
                }
                let dest =
                    from_raw_parts_mut(ptr.cast::<u8>().add(offset.align_offset), offset.len);
                let src = from_raw_parts(data_ptr, offset.len);
                dest.copy_from_slice(src);
                let res = mprotect(ptr, len, prot.bits());
                if res != 0 {
                    return Err(map_error("mprotect failed"));
                }
                Ok(NonNull::new_unchecked(ptr))
            }
        }
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: super::ProtFlags,
        flags: super::MapFlags,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let ptr = mmap(addr as _, len, prot.bits(), flags.bits(), -1, 0);
        if ptr == libc::MAP_FAILED {
            return Err(map_error("mmap anonymous failed"));
        }
        Ok(NonNull::new_unchecked(ptr))
    }

    unsafe fn munmap(addr: core::ptr::NonNull<core::ffi::c_void>, len: usize) -> crate::Result<()> {
        let res = munmap(addr.as_ptr(), len);
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
        let res = mprotect(addr.as_ptr(), len, prot.bits());
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
