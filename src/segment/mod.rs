//! The Memory mapping of elf object
pub(crate) mod phdr;
pub(crate) mod shdr;

use super::mmap::{self, Mmap, ProtFlags};
use crate::{
    Result,
    arch::Phdr,
    mmap::MapFlags,
    object::{ElfObject, ElfObjectAsync},
};
use alloc::vec::Vec;
use core::ffi::c_void;
use core::fmt::Debug;
use core::ptr::NonNull;

pub const PAGE_SIZE: usize = 0x1000;
pub const MASK: usize = !(PAGE_SIZE - 1);

enum Address {
    Relative(usize),
    Absolute(usize),
}

impl Address {
    fn absolute_addr(&self) -> usize {
        match self {
            Address::Relative(_) => unreachable!(),
            Address::Absolute(addr) => *addr,
        }
    }

    fn relative_addr(&self) -> usize {
        match self {
            Address::Relative(addr) => *addr,
            Address::Absolute(_) => unreachable!(),
        }
    }
}

pub(crate) struct ElfSegment {
    addr: Address,
    prot: ProtFlags,
    flags: MapFlags,
    len: usize,
    zero_size: usize,
    content_size: usize,
    map_info: Vec<FileMapInfo>,
    need_copy: bool,
    from_relocatable: bool,
}

impl ElfSegment {
    fn rebase(&mut self, base: usize) {
        self.addr = Address::Absolute(base + self.addr.relative_addr());
    }

    fn mmap_segment<M: Mmap>(&mut self, object: &mut impl ElfObject) -> Result<()> {
        let mut need_copy = false;
        let len = self.len;
        let addr = self.addr.absolute_addr();
        let prot = if self.from_relocatable {
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE
        } else {
            self.prot
        };
        debug_assert!(len % PAGE_SIZE == 0);
        if self.map_info.len() == 1 {
            debug_assert!(self.map_info[0].offset % PAGE_SIZE == 0);
            unsafe {
                M::mmap(
                    Some(addr),
                    len,
                    prot,
                    self.flags,
                    self.map_info[0].offset,
                    object.as_fd(),
                    &mut need_copy,
                )
            }?
        } else {
            unsafe { M::mmap(Some(addr), len, prot, self.flags, 0, None, &mut need_copy) }?
        };
        #[cfg(feature = "log")]
        log::trace!(
            "[Mmap] address: 0x{:x}, length: {}, flags: {:?}, zero_size: {}, map_info: {:?}",
            addr,
            len,
            prot,
            self.zero_size,
            self.map_info
        );
        self.need_copy = need_copy;
        Ok(())
    }

    fn copy_data(&self, object: &mut impl ElfObject) -> Result<()> {
        if self.need_copy {
            let ptr = self.addr.absolute_addr() as *mut u8;
            for info in self.map_info.iter() {
                unsafe {
                    let dest = core::slice::from_raw_parts_mut(ptr.add(info.start), info.filesz);
                    object.read(dest, info.offset)?;
                }
            }
        }
        Ok(())
    }

    async fn copy_data_async(&self, object: &mut impl ElfObjectAsync) -> Result<()> {
        if self.need_copy {
            let ptr = self.addr.absolute_addr() as *mut u8;
            for info in self.map_info.iter() {
                unsafe {
                    let dest = core::slice::from_raw_parts_mut(ptr.add(info.start), info.filesz);
                    object.read_async(dest, info.offset).await?;
                }
            }
        }
        Ok(())
    }

    fn mprotect<M: Mmap>(&self) -> Result<()> {
        if self.need_copy || self.from_relocatable {
            let len = self.len;
            debug_assert!(len % PAGE_SIZE == 0);
            let addr = self.addr.absolute_addr();
            unsafe { M::mprotect(NonNull::new(addr as _).unwrap(), len, self.prot) }?;
            #[cfg(feature = "log")]
            log::trace!(
                "[Mprotect] address: 0x{:x}, length: {}, prot: {:?}",
                addr,
                len,
                self.prot,
            );
        }
        Ok(())
    }

    fn fill_zero<M: Mmap>(&self) -> Result<()> {
        if self.zero_size > 0 {
            // 用0填充这一页
            let zero_start = self.addr.absolute_addr() + self.content_size;
            let zero_end = roundup(zero_start, PAGE_SIZE);
            let wirte_len = zero_end - zero_start;
            let ptr = zero_start as *mut u8;
            unsafe {
                ptr.write_bytes(0, wirte_len);
            };

            if wirte_len < self.zero_size {
                //之后剩余的一定是页的整数倍
                //如果有剩余的页的话，将其映射为匿名页
                let zero_mmap_addr = zero_end;
                let zero_mmap_len = self.zero_size - wirte_len;
                unsafe {
                    M::mmap_anonymous(
                        zero_mmap_addr,
                        zero_mmap_len,
                        self.prot,
                        mmap::MapFlags::MAP_PRIVATE | mmap::MapFlags::MAP_FIXED,
                    )?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) trait SegmentBuilder {
    fn create_space<M: Mmap>(&mut self) -> Result<ElfSegments>;

    fn create_segments(&mut self) -> Result<()>;

    fn segments_mut(&mut self) -> &mut [ElfSegment];

    fn segments(&self) -> &[ElfSegment];

    fn load_segments<M: Mmap>(&mut self, object: &mut impl ElfObject) -> Result<ElfSegments> {
        let space = self.create_space::<M>()?;
        self.create_segments()?;
        let segments = self.segments_mut();
        let base = space.base();
        for segment in segments.iter_mut() {
            segment.rebase(base);
            // if object.as_fd().is_some() {
            //     if segment.addr.absolute_addr() + segment.total_size != space.base() + space.len() {
            //         let len = param.addr + param.len - *last_address;
            //         crate::os::virtual_free(*last_address, len)?;
            //         *last_address = param.addr + param.len;
            //     }
            // }
            segment.mmap_segment::<M>(object)?;
            segment.copy_data(object)?;
            segment.fill_zero::<M>()?;
        }
        Ok(space)
    }

    async fn load_segments_async<M: Mmap>(
        &mut self,
        object: &mut impl ElfObjectAsync,
    ) -> Result<ElfSegments> {
        let space = self.create_space::<M>()?;
        self.create_segments()?;
        let segments = self.segments_mut();
        let base = space.base();
        for segment in segments.iter_mut() {
            segment.rebase(base);
            // if object.as_fd().is_some() {
            //     if segment.addr.absolute_addr() + segment.total_size != space.base() + space.len() {
            //         let len = param.addr + param.len - *last_address;
            //         crate::os::virtual_free(*last_address, len)?;
            //         *last_address = param.addr + param.len;
            //     }
            // }
            segment.mmap_segment::<M>(object)?;
            segment.copy_data_async(object).await?;
            segment.fill_zero::<M>()?;
        }
        Ok(space)
    }

    fn mprotect<M: Mmap>(&self) -> Result<()> {
        let segments = self.segments();
        for segment in segments.iter() {
            segment.mprotect::<M>()?;
        }
        Ok(())
    }
}

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

#[derive(Debug)]
struct FileMapInfo {
    start: usize,
    filesz: usize,
    offset: usize,
}

#[inline]
fn roundup(x: usize, align: usize) -> usize {
    if align == 0 {
        return x;
    }
    (x + align - 1) & !(align - 1)
}

#[inline]
fn rounddown(x: usize, align: usize) -> usize {
    x & !(align - 1)
}

// async fn mmap_segment_async<M: Mmap>(
//     param: &MmapParam,
//     object: &mut impl ElfObjectAsync,
// ) -> Result<NonNull<c_void>> {
//     let mut need_copy = false;
//     let ptr = unsafe {
//         M::mmap(
//             Some(param.addr),
//             param.len,
//             param.prot,
//             param.flags,
//             param.file.offset,
//             object.as_fd(),
//             &mut need_copy,
//         )
//     }?;
//     if need_copy {
//         unsafe {
//             let dest =
//                 core::slice::from_raw_parts_mut(ptr.as_ptr().cast::<u8>(), param.file.filesz);
//             object.read_async(dest, param.file.offset).await?;
//             M::mprotect(ptr, param.len, param.prot)?;
//         }
//     }
//     Ok(ptr)
// }

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
        let end = roundup(self.addr + self.len, PAGE_SIZE);
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

    // pub(crate) fn load_segment<M: Mmap>(
    //     &mut self,
    //     object: &mut impl ElfObject,
    //     phdr: &Phdr,
    //     #[allow(unused)] last_address: &mut usize,
    // ) -> Result<()> {
    //     let param = parse_segment(self, phdr);
    //     #[cfg(target_os = "windows")]
    //     if object.as_fd().is_some() {
    //         if self.memory.as_ptr() as usize + self.len != param.addr + param.len {
    //             let len = param.addr + param.len - *last_address;
    //             crate::os::virtual_free(*last_address, len)?;
    //             *last_address = param.addr + param.len;
    //         }
    //     }
    //     mmap_segment::<M>(&param, object)?;
    //     self.fill_bss::<M>(phdr)?;
    //     Ok(())
    // }

    // pub(crate) async fn load_segment_async<M: Mmap>(
    //     &mut self,
    //     object: &mut impl ElfObjectAsync,
    //     phdr: &Phdr,
    //     #[allow(unused)] last_address: &mut usize,
    // ) -> Result<()> {
    //     // let param = parse_segment(self, phdr);
    //     // #[cfg(target_os = "windows")]
    //     // if object.as_fd().is_some() {
    //     //     if self.memory.as_ptr() as usize + self.len != param.addr + param.len {
    //     //         let len = param.addr + param.len - *last_address;
    //     //         crate::os::virtual_free(*last_address, len)?;
    //     //         *last_address = param.addr + param.len;
    //     //     }
    //     // }
    //     // mmap_segment_async::<M>(&param, object).await?;
    //     // self.fill_bss::<M>(phdr)?;
    //     Ok(())
    // }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// len以byte为单位
    #[inline]
    pub(crate) fn get_slice<T>(&self, start: usize, len: usize) -> &'static [T] {
        unsafe {
            // 保证切片在可映射的elf段内
            debug_assert!(start + len - self.offset <= self.len);
            core::slice::from_raw_parts(self.get_ptr::<T>(start), len / size_of::<T>())
        }
    }

    /// len以byte为单位
    pub(crate) fn get_slice_mut<T>(&self, start: usize, len: usize) -> &'static mut [T] {
        unsafe {
            // 保证切片在可映射的elf段内
            debug_assert!(start + len - self.offset <= self.len);
            core::slice::from_raw_parts_mut(self.get_mut_ptr::<T>(start), len / size_of::<T>())
        }
    }

    #[inline]
    pub(crate) fn get_ptr<T>(&self, offset: usize) -> *const T {
        // 保证offset在可映射的elf段内
        debug_assert!(offset - self.offset < self.len);
        (self.base() + offset) as *const T
    }

    #[inline]
    pub(crate) fn get_mut_ptr<T>(&self, offset: usize) -> *mut T {
        self.get_ptr::<T>(offset) as *mut T
    }

    /// base = memory_addr - offset
    #[inline]
    pub fn base(&self) -> usize {
        unsafe { self.memory.as_ptr().cast::<u8>().sub(self.offset) as usize }
    }
}
