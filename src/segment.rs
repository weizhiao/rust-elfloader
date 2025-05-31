//! The Memory mapping of elf object
use super::mmap::{self, Mmap, ProtFlags};
use crate::{
    Result,
    arch::{ElfPhdr, Phdr},
    mmap::MapFlags,
    object::{ElfObject, ElfObjectAsync},
};
use core::ffi::c_void;
use core::fmt::Debug;
use core::ptr::NonNull;
use elf::abi::{PF_R, PF_W, PF_X, PT_LOAD};

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

struct FileMapInfo {
    filesz: usize,
    offset: usize,
}

struct MmapParam {
    addr: Option<usize>,
    len: usize,
    prot: ProtFlags,
    flags: MapFlags,
    file: FileMapInfo,
}

#[inline]
fn map_prot(prot: u32) -> mmap::ProtFlags {
    mmap::ProtFlags::from_bits_retain(((prot & PF_X) << 2 | prot & PF_W | (prot & PF_R) >> 2) as _)
}

#[inline]
fn roundup(x: usize) -> usize {
    (x + PAGE_SIZE - 1) & MASK
}

#[inline]
fn rounddown(x: usize) -> usize {
    x & MASK
}

fn mmap_segment<M: Mmap>(
    param: &MmapParam,
    object: &mut impl ElfObject,
) -> Result<NonNull<c_void>> {
    let mut need_copy = false;
    let ptr = unsafe {
        M::mmap(
            param.addr,
            param.len,
            param.prot,
            param.flags,
            param.file.offset,
            object.as_fd(),
            &mut need_copy,
        )
    }?;
    if need_copy {
        unsafe {
            let dest =
                core::slice::from_raw_parts_mut(ptr.as_ptr().cast::<u8>(), param.file.filesz);
            object.read(dest, param.file.offset)?;
            M::mprotect(ptr, param.len, param.prot)?;
        }
    }
    Ok(ptr)
}

async fn mmap_segment_async<M: Mmap>(
    param: &MmapParam,
    object: &mut impl ElfObjectAsync,
) -> Result<NonNull<c_void>> {
    let mut need_copy = false;
    let ptr = unsafe {
        M::mmap(
            param.addr,
            param.len,
            param.prot,
            param.flags,
            param.file.offset,
            object.as_fd(),
            &mut need_copy,
        )
    }?;
    if need_copy {
        unsafe {
            let dest =
                core::slice::from_raw_parts_mut(ptr.as_ptr().cast::<u8>(), param.file.filesz);
            object.read(dest, param.file.offset)?;
            M::mprotect(ptr, param.len, param.prot)?;
        }
    }
    Ok(ptr)
}

#[inline]
fn parse_segments(phdrs: &[ElfPhdr], is_dylib: bool) -> (MmapParam, usize, usize) {
    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0;
    // 最小偏移地址对应内容在文件中的偏移
    let mut min_off = 0;
    let mut min_filesz = 0;
    let mut min_prot = 0;
    let mut min_memsz = 0;

    //找到最小的偏移地址和最大的偏移地址
    for phdr in phdrs {
        if phdr.p_type == PT_LOAD {
            let vaddr_start = phdr.p_vaddr as usize;
            let vaddr_end = (phdr.p_vaddr + phdr.p_memsz) as usize;
            if vaddr_start < min_vaddr {
                min_vaddr = vaddr_start;
                min_off = phdr.p_offset as usize;
                min_prot = phdr.p_flags;
                min_filesz = phdr.p_filesz as usize;
                min_memsz = phdr.p_memsz as usize;
            }
            if vaddr_end > max_vaddr {
                max_vaddr = vaddr_end;
            }
        }
    }

    // 按页对齐
    max_vaddr = roundup(max_vaddr);
    min_vaddr = rounddown(min_vaddr);
    let total_size = max_vaddr - min_vaddr;
    let prot = map_prot(min_prot);
    (
        MmapParam {
            addr: if is_dylib { None } else { Some(min_vaddr) },
            len: total_size,
            prot,
            flags: mmap::MapFlags::MAP_PRIVATE,
            file: FileMapInfo {
                filesz: min_filesz,
                offset: min_off,
            },
        },
        min_vaddr,
        roundup(min_vaddr + min_memsz),
    )
}

#[inline]
fn parse_segment(segments: &ElfSegments, phdr: &Phdr) -> Option<MmapParam> {
    let addr_min = segments.offset();
    let base = segments.base();
    // 映射的起始地址与结束地址都是页对齐的
    let min_vaddr = rounddown(phdr.p_vaddr as usize);
    let max_vaddr = roundup((phdr.p_vaddr + phdr.p_memsz) as usize);
    let memsz = max_vaddr - min_vaddr;
    let prot = map_prot(phdr.p_flags);
    let real_addr = min_vaddr + base;
    let offset = rounddown(phdr.p_offset as usize);
    // 因为读取是从offset处开始的，所以为了不少从文件中读数据，这里需要加上因为对齐产生的偏差
    let align_len = phdr.p_offset as usize - offset;
    let filesz = phdr.p_filesz as usize + align_len;
    // 这是一个优化，可以减少一次mmap调用。
    // 映射create_segments产生的参数时会将处于最低地址处的segment也映射进去，所以这里不需要在映射它
    if addr_min != min_vaddr {
        Some(MmapParam {
            addr: Some(real_addr),
            len: memsz,
            prot,
            flags: mmap::MapFlags::MAP_PRIVATE | mmap::MapFlags::MAP_FIXED,
            file: FileMapInfo { filesz, offset },
        })
    } else {
        None
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
        let end = roundup(self.addr + self.len);
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

    pub(crate) fn create_segments<M: Mmap>(
        object: &mut impl ElfObject,
        phdrs: &[ElfPhdr],
        is_dylib: bool,
    ) -> Result<Self> {
        let (param, min_vaddr, min_memsz) = parse_segments(phdrs, is_dylib);
        let mut need_copy = false;
        let ptr = unsafe {
            M::mmap(
                param.addr,
                param.len,
                param.prot,
                param.flags,
                param.file.offset,
                object.as_fd(),
                &mut need_copy,
            )
        }?;
        if need_copy {
            unsafe {
                let dest =
                    core::slice::from_raw_parts_mut(ptr.as_ptr().cast::<u8>(), param.file.filesz);
                object.read(dest, param.file.offset)?;
                M::mprotect(ptr, min_memsz, param.prot)?;
            }
        }
        Ok(ElfSegments {
            memory: ptr,
            offset: min_vaddr,
            len: param.len,
            munmap: M::munmap,
        })
    }

    pub(crate) async fn create_segments_async<M: Mmap>(
        object: &mut impl ElfObjectAsync,
        phdrs: &[ElfPhdr],
        is_dylib: bool,
    ) -> Result<Self> {
        let (param, min_vaddr, min_memsz) = parse_segments(phdrs, is_dylib);
        let mut need_copy = false;
        let ptr = unsafe {
            M::mmap(
                param.addr,
                param.len,
                param.prot,
                param.flags,
                param.file.offset,
                object.as_fd(),
                &mut need_copy,
            )
        }?;
        if need_copy {
            unsafe {
                let dest =
                    core::slice::from_raw_parts_mut(ptr.as_ptr().cast::<u8>(), param.file.filesz);
                object.read_async(dest, param.file.offset).await?;
                M::mprotect(ptr, min_memsz, param.prot)?;
            }
        }
        Ok(ElfSegments {
            memory: ptr,
            offset: min_vaddr,
            len: param.len,
            munmap: M::munmap,
        })
    }

    pub(crate) fn load_segment<M: Mmap>(
        &mut self,
        object: &mut impl ElfObject,
        phdr: &Phdr,
    ) -> Result<()> {
        if let Some(param) = parse_segment(self, phdr) {
            mmap_segment::<M>(&param, object)?;
            self.fill_bss::<M>(phdr)?;
        }
        Ok(())
    }

    pub(crate) async fn load_segment_async<M: Mmap>(
        &mut self,
        object: &mut impl ElfObjectAsync,
        phdr: &Phdr,
    ) -> Result<()> {
        if let Some(param) = parse_segment(self, phdr) {
            mmap_segment_async::<M>(&param, object).await?;
            self.fill_bss::<M>(phdr)?;
        }
        Ok(())
    }

    fn fill_bss<M: Mmap>(&self, phdr: &Phdr) -> Result<()> {
        if phdr.p_filesz != phdr.p_memsz {
            let prot = map_prot(phdr.p_flags);
            let max_vaddr = roundup(phdr.p_vaddr as usize + phdr.p_memsz as usize);
            // 用0填充这一页
            let zero_start = (phdr.p_vaddr + phdr.p_filesz) as usize;
            let zero_end = roundup(zero_start);
            unsafe {
                self.get_mut_ptr::<u8>(zero_start)
                    .write_bytes(0, zero_end - zero_start);
            };

            if zero_end < max_vaddr {
                //之后剩余的一定是页的整数倍
                //如果有剩余的页的话，将其映射为匿名页
                let zero_mmap_addr = self.base() + zero_end;
                let zero_mmap_len = max_vaddr - zero_end;
                unsafe {
                    M::mmap_anonymous(
                        zero_mmap_addr,
                        zero_mmap_len,
                        prot,
                        mmap::MapFlags::MAP_PRIVATE | mmap::MapFlags::MAP_FIXED,
                    )?;
                }
            }
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

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
