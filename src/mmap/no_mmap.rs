use super::Mmap;
use crate::segment::PAGE_SIZE;
use alloc::alloc::{dealloc, handle_alloc_error};
use core::{
    alloc::Layout,
    ptr::NonNull,
    slice::{from_raw_parts, from_raw_parts_mut},
};

/// An implementation of Mmap trait
pub struct MmapImpl;

impl Mmap for MmapImpl {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        _prot: super::ProtFlags,
        flags: super::MapFlags,
        offset: super::MmapOffset,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        match (offset.kind, addr) {
            #[cfg(feature = "std")]
            (super::OffsetType::File { fd, file_offset }, None) => {
                use std::io::{Read, Seek};
                use std::os::fd::FromRawFd;
                // 只有创建整个空间时会走这条路径
                assert!((super::MapFlags::MAP_FIXED & flags).bits() == 0);
                let total_size = len + PAGE_SIZE;
                let layout = Layout::from_size_align_unchecked(total_size, PAGE_SIZE);
                let memory = alloc::alloc::alloc(layout);
                if memory.is_null() {
                    handle_alloc_error(layout);
                }
                let dest = from_raw_parts_mut(memory, offset.len);
                let mut file = std::fs::File::from_raw_fd(fd);
                file.seek(std::io::SeekFrom::Start(file_offset as _))?;
                file.read_exact(dest)?;
                // 防止提前关闭file
                core::mem::forget(file);
                // use this set prot to test no_mmap
                // libc::mprotect(memory as _, len, crate::mmap::ProtFlags::all().bits());
                Ok(NonNull::new_unchecked(memory as _))
            }
            #[cfg(feature = "std")]
            (super::OffsetType::File { fd, file_offset }, Some(addr)) => {
                use std::io::{Read, Seek};
                use std::os::fd::FromRawFd;
                let ptr = addr as *mut u8;
                let dest = from_raw_parts_mut(ptr, offset.len);
                let mut file = std::fs::File::from_raw_fd(fd);
                file.seek(std::io::SeekFrom::Start(file_offset as _))?;
                file.read_exact(dest)?;
                // 防止提前关闭file
                core::mem::forget(file);
                Ok(NonNull::new_unchecked(ptr as _))
            }
            (super::OffsetType::Addr(data_ptr), None) => {
                // 只有创建整个空间时会走这条路径
                assert!((super::MapFlags::MAP_FIXED & flags).bits() == 0);
                let total_size = len + PAGE_SIZE;
                let layout = Layout::from_size_align_unchecked(total_size, PAGE_SIZE);
                let memory = alloc::alloc::alloc(layout);
                if memory.is_null() {
                    handle_alloc_error(layout);
                }
                let dest = from_raw_parts_mut(memory, offset.len);
                let src = from_raw_parts(data_ptr, offset.len);
                dest.copy_from_slice(src);
                // use this set prot to test no_mmap
                // libc::mprotect(memory as _, len, crate::mmap::ProtFlags::all().bits());
                Ok(NonNull::new_unchecked(memory as _))
            }
            (super::OffsetType::Addr(data_ptr), Some(addr)) => {
                let ptr = addr as *mut u8;
                let dest = from_raw_parts_mut(ptr, offset.len);
                let src = from_raw_parts(data_ptr, offset.len);
                dest.copy_from_slice(src);
                Ok(NonNull::new_unchecked(ptr as _))
            }
            #[allow(unused)]
            _ => {
                unreachable!()
            }
        }
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        _prot: super::ProtFlags,
        _flags: super::MapFlags,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let ptr = addr as *mut u8;
        let dest = from_raw_parts_mut(ptr, len);
        dest.fill(0);
        Ok(NonNull::new_unchecked(ptr as _))
    }

    unsafe fn munmap(addr: core::ptr::NonNull<core::ffi::c_void>, len: usize) -> crate::Result<()> {
        dealloc(
            addr.as_ptr() as _,
            Layout::from_size_align_unchecked(len, PAGE_SIZE),
        );
        Ok(())
    }

    unsafe fn mprotect(
        _addr: NonNull<core::ffi::c_void>,
        _len: usize,
        _prot: super::ProtFlags,
    ) -> crate::Result<()> {
        Ok(())
    }
}
