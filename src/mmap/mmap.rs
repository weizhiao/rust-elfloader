use super::Mmap;
use crate::segment::MASK;
use crate::Error;
use core::slice::{from_raw_parts, from_raw_parts_mut};
use core::{ffi::c_int, num::NonZeroUsize};
use nix::sys::mman;
use std::os::fd::{AsFd, BorrowedFd};

pub struct MmapImpl;

struct Fd(c_int);

impl AsFd for Fd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.0) }
    }
}

impl Mmap for MmapImpl {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: super::ProtFlags,
        flags: super::MapFlags,
        offset: super::Offset,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let addr = addr.map(|val| NonZeroUsize::new_unchecked(val));
        let length = NonZeroUsize::new_unchecked(len);
        let prot = mman::ProtFlags::from_bits(prot.bits()).unwrap();
        let flags = mman::MapFlags::from_bits(flags.bits()).unwrap();
        match offset.kind {
            super::OffsetType::File { fd, file_offset } => Ok(mman::mmap(
                addr,
                length,
                prot,
                flags,
                Fd(fd),
                // offset是当前段在文件中的偏移，需要按照页对齐，否则mmap会失败
                (file_offset & MASK) as _,
            )?),
            super::OffsetType::Addr(data_ptr) => {
                let ptr = mman::mmap_anonymous(addr, length, mman::ProtFlags::PROT_WRITE, flags)?;
                let dest = from_raw_parts_mut(
                    ptr.as_ptr().cast::<u8>().add(offset.align_offset),
                    offset.len,
                );
                let src = from_raw_parts(data_ptr, offset.len);
                dest.copy_from_slice(src);
                mman::mprotect(ptr, length.into(), prot)?;
                Ok(ptr)
            }
        }
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: super::ProtFlags,
        flags: super::MapFlags,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        Ok(mman::mmap_anonymous(
            Some(NonZeroUsize::new_unchecked(addr)),
            NonZeroUsize::new_unchecked(len),
            mman::ProtFlags::from_bits(prot.bits()).unwrap(),
            mman::MapFlags::from_bits(flags.bits()).unwrap(),
        )?)
    }

    unsafe fn munmap(addr: core::ptr::NonNull<core::ffi::c_void>, len: usize) -> crate::Result<()> {
        Ok(mman::munmap(addr, len)?)
    }

    unsafe fn mprotect(
        addr: core::ptr::NonNull<core::ffi::c_void>,
        len: usize,
        prot: super::ProtFlags,
    ) -> crate::Result<()> {
        mman::mprotect(addr, len, mman::ProtFlags::from_bits(prot.bits()).unwrap())?;
        Ok(())
    }
}

impl From<nix::Error> for Error {
    #[cold]
    fn from(value: nix::Error) -> Self {
        Error::MmapError {
            msg: value.to_string(),
        }
    }
}
