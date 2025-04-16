use crate::{
    ElfObject, Result, UserData,
    arch::{Dyn, E_CLASS, EHDR_SIZE, EM_ARCH, Ehdr, ElfPhdr, Phdr},
    format::InitParam,
    mmap::{self, MapFlags, Mmap, ProtFlags},
    object::ElfObjectAsync,
    parse_ehdr_error, parse_phdr_error,
    segment::{ELFRelro, ElfSegments, MASK, PAGE_SIZE},
};
use alloc::{borrow::ToOwned, boxed::Box, ffi::CString, format, vec::Vec};
use core::{
    any::Any,
    ffi::{CStr, c_void},
    marker::PhantomData,
    ops::Deref,
    ptr::NonNull,
};
use elf::abi::{
    EI_CLASS, EI_VERSION, ELFMAGIC, ET_DYN, EV_CURRENT, PT_DYNAMIC, PT_GNU_RELRO, PT_INTERP,
    PT_LOAD, PT_PHDR,
};

#[repr(transparent)]
pub struct ElfHeader {
    ehdr: Ehdr,
}

impl Clone for ElfHeader {
    fn clone(&self) -> Self {
        Self {
            ehdr: Ehdr {
                e_ident: self.e_ident,
                e_type: self.e_type,
                e_machine: self.e_machine,
                e_version: self.e_version,
                e_entry: self.e_entry,
                e_phoff: self.e_phoff,
                e_shoff: self.e_shoff,
                e_flags: self.e_flags,
                e_ehsize: self.e_ehsize,
                e_phentsize: self.e_phentsize,
                e_phnum: self.e_phnum,
                e_shentsize: self.e_shentsize,
                e_shnum: self.e_shnum,
                e_shstrndx: self.e_shstrndx,
            },
        }
    }
}

impl Deref for ElfHeader {
    type Target = Ehdr;

    fn deref(&self) -> &Self::Target {
        &self.ehdr
    }
}

impl ElfHeader {
    pub(crate) fn new(data: &[u8]) -> Result<&Self> {
        debug_assert!(data.len() >= EHDR_SIZE);
        let ehdr: &ElfHeader = unsafe { &*(data.as_ptr().cast()) };
        ehdr.vaildate()?;
        Ok(ehdr)
    }

    #[inline]
    pub fn is_dylib(&self) -> bool {
        self.ehdr.e_type == ET_DYN
    }

    pub(crate) fn vaildate(&self) -> Result<()> {
        if self.e_ident[0..4] != ELFMAGIC {
            return Err(parse_ehdr_error("invalid ELF magic"));
        }
        if self.e_ident[EI_CLASS] != E_CLASS {
            return Err(parse_ehdr_error("file class mismatch"));
        }
        if self.e_ident[EI_VERSION] != EV_CURRENT {
            return Err(parse_ehdr_error("invalid ELF version"));
        }
        if self.e_machine != EM_ARCH {
            return Err(parse_ehdr_error("file arch mismatch"));
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn e_phnum(&self) -> usize {
        self.ehdr.e_phnum as usize
    }

    #[inline]
    pub(crate) fn e_phentsize(&self) -> usize {
        self.ehdr.e_phentsize as usize
    }

    #[inline]
    pub(crate) fn e_phoff(&self) -> usize {
        self.ehdr.e_phoff as usize
    }

    #[inline]
    pub(crate) fn phdr_range(&self) -> (usize, usize) {
        let phdrs_size = self.e_phentsize() * self.e_phnum();
        let phdr_start = self.e_phoff();
        let phdr_end = phdr_start + phdrs_size;
        (phdr_start, phdr_end)
    }
}

/// This struct is used to specify the offset and length for memory-mapped regions.
struct MmapRange {
    /// The length of the memory region to be mapped.
    len: usize,
    /// The offset of the mapped region in the elf object. It is always aligned by page size(4096 or 65536).
    offset: usize,
}

struct MmapParam {
    addr: Option<usize>,
    len: usize,
    prot: ProtFlags,
    flags: MapFlags,
    range: MmapRange,
}

#[inline(always)]
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
            param.range.offset,
            object.as_fd(),
            &mut need_copy,
        )
    }?;
    if need_copy {
        unsafe {
            let dest = core::slice::from_raw_parts_mut(ptr.as_ptr().cast::<u8>(), param.range.len);
            object.read(dest, param.range.offset)?;
            M::mprotect(ptr, param.len, param.prot)?;
        }
    }
    Ok(ptr)
}

#[inline(always)]
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
            param.range.offset,
            object.as_fd(),
            &mut need_copy,
        )
    }?;
    if need_copy {
        let dest =
            unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr().cast::<u8>(), param.range.len) };
        object.read_async(dest, param.range.offset).await?;
        unsafe { M::mprotect(ptr, param.len, param.prot) }?;
    }
    Ok(ptr)
}

#[inline]
fn create_segments(phdrs: &[ElfPhdr], is_dylib: bool) -> (MmapParam, usize) {
    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0;
    // 最小偏移地址对应内容在文件中的偏移
    let mut min_off = 0;
    let mut min_filesz = 0;
    let mut min_prot = 0;

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
            }
            if vaddr_end > max_vaddr {
                max_vaddr = vaddr_end;
            }
        }
    }

    // 按页对齐
    max_vaddr = (max_vaddr + PAGE_SIZE - 1) & MASK;
    min_vaddr &= MASK;
    let total_size = max_vaddr - min_vaddr;
    let prot = ElfSegments::map_prot(min_prot);
    (
        MmapParam {
            addr: if is_dylib { None } else { Some(min_vaddr) },
            len: total_size,
            prot,
            flags: mmap::MapFlags::MAP_PRIVATE,
            range: MmapRange {
                len: min_filesz,
                offset: min_off,
            },
        },
        min_vaddr,
    )
}

#[inline]
fn load_segment(segments: &ElfSegments, phdr: &Phdr) -> Option<MmapParam> {
    let addr_min = segments.offset();
    let base = segments.base();
    // 映射的起始地址与结束地址都是页对齐的
    let min_vaddr = phdr.p_vaddr as usize & MASK;
    let max_vaddr = (phdr.p_vaddr as usize + phdr.p_memsz as usize + PAGE_SIZE - 1) & MASK;
    let memsz = max_vaddr - min_vaddr;
    let prot = ElfSegments::map_prot(phdr.p_flags);
    let real_addr = min_vaddr + base;
    let offset = phdr.p_offset as usize & MASK;
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
            range: MmapRange {
                len: filesz,
                offset,
            },
        })
    } else {
        None
    }
}

#[inline]
fn fill_bss<M: Mmap>(segments: &mut ElfSegments, phdr: &Phdr) -> Result<()> {
    if phdr.p_filesz != phdr.p_memsz {
        let prot = ElfSegments::map_prot(phdr.p_flags);
        let max_vaddr = (phdr.p_vaddr as usize + phdr.p_memsz as usize + PAGE_SIZE - 1) & MASK;
        // 用0填充这一页
        let zero_start = (phdr.p_vaddr + phdr.p_filesz) as usize;
        let zero_end = (zero_start + PAGE_SIZE - 1) & MASK;
        unsafe {
            segments
                .get_mut_ptr::<u8>(zero_start)
                .write_bytes(0, zero_end - zero_start);
        };

        if zero_end < max_vaddr {
            //之后剩余的一定是页的整数倍
            //如果有剩余的页的话，将其映射为匿名页
            let zero_mmap_addr = segments.base() + zero_end;
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

pub(crate) struct Builder {
    pub(crate) phdr_mmap: Option<&'static [ElfPhdr]>,
    pub(crate) name: CString,
    pub(crate) lazy_bind: Option<bool>,
    pub(crate) ehdr: ElfHeader,
    pub(crate) relro: Option<ELFRelro>,
    pub(crate) dynamic_ptr: Option<NonNull<Dyn>>,
    pub(crate) user_data: UserData,
    pub(crate) segments: ElfSegments,
    pub(crate) init_param: Option<InitParam>,
    pub(crate) interp: Option<&'static str>,
}

impl Builder {
    const fn new(
        segments: ElfSegments,
        name: CString,
        lazy_bind: Option<bool>,
        ehdr: ElfHeader,
        init_param: Option<InitParam>,
    ) -> Self {
        Self {
            phdr_mmap: None,
            name,
            lazy_bind,
            ehdr,
            relro: None,
            dynamic_ptr: None,
            segments,
            user_data: UserData::empty(),
            init_param,
            interp: None,
        }
    }

    fn exec_hook(&mut self, hook: &Hook, phdr: &ElfPhdr) -> Result<()> {
        hook(&self.name, phdr, &self.segments, &mut self.user_data).map_err(|err| {
            parse_phdr_error(
                format!(
                    "failed to execute the hook function on dylib: {}",
                    self.name.to_str().unwrap()
                ),
                err,
            )
        })?;
        Ok(())
    }

    fn parse_other_phdr<M: Mmap>(&mut self, phdr: &Phdr) {
        match phdr.p_type {
            // 解析.dynamic section
            PT_DYNAMIC => {
                self.dynamic_ptr =
                    Some(NonNull::new(self.segments.get_mut_ptr(phdr.p_paddr as usize)).unwrap())
            }
            PT_GNU_RELRO => self.relro = Some(ELFRelro::new::<M>(phdr, self.segments.base())),
            PT_PHDR => {
                self.phdr_mmap = Some(
                    self.segments
                        .get_slice::<ElfPhdr>(phdr.p_vaddr as usize, phdr.p_memsz as usize),
                );
            }
            PT_INTERP => {
                self.interp = Some(unsafe {
                    CStr::from_ptr(self.segments.get_ptr(phdr.p_vaddr as usize))
                        .to_str()
                        .unwrap()
                });
            }
            _ => {}
        };
    }
}

pub(crate) struct ElfBuf {
    buf: Vec<u8>,
}

impl ElfBuf {
    fn new() -> Self {
        let mut buf = Vec::new();
        buf.resize(EHDR_SIZE, 0);
        ElfBuf { buf }
    }

    pub(crate) fn prepare_ehdr(&mut self, object: &mut impl ElfObject) -> Result<ElfHeader> {
        object.read(&mut self.buf[..EHDR_SIZE], 0)?;
        ElfHeader::new(&self.buf).cloned()
    }

    pub(crate) fn prepare_phdr(
        &mut self,
        ehdr: &ElfHeader,
        object: &mut impl ElfObject,
    ) -> Result<&[ElfPhdr]> {
        let (phdr_start, phdr_end) = ehdr.phdr_range();
        let size = phdr_end - phdr_start;
        if size > self.buf.len() {
            self.buf.resize(size, 0);
        }
        object.read(&mut self.buf[..size], phdr_start)?;
		unsafe {
            Ok(core::slice::from_raw_parts(
                self.buf.as_ptr().cast::<ElfPhdr>(),
                self.buf.len() / size_of::<ElfPhdr>(),
            ))
        }
    }
}

pub(crate) type Hook = Box<
    dyn Fn(&CStr, &ElfPhdr, &ElfSegments, &mut UserData) -> core::result::Result<(), Box<dyn Any>>,
>;

/// The elf object loader
pub struct Loader<M>
where
    M: Mmap,
{
    pub(crate) init_param: Option<InitParam>,
    pub(crate) buf: ElfBuf,
    hook: Option<Hook>,
    _marker: PhantomData<M>,
}

impl<M: Mmap> Default for Loader<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Mmap> Loader<M> {
    /// Create a new loader
    pub fn new() -> Self {
        Self {
            init_param: None,
            hook: None,
            buf: ElfBuf::new(),
            _marker: PhantomData,
        }
    }

    /// glibc passes argc, argv, and envp to functions in .init_array, as a non-standard extension.
    pub fn set_init_params(&mut self, argc: usize, argv: usize, envp: usize) {
        self.init_param = Some(InitParam { argc, argv, envp });
    }

    /// `hook` functions are called first when a program header is processed
    pub fn set_hook(&mut self, hook: Hook) {
        self.hook = Some(hook)
    }

    pub fn read_ehdr(&mut self, object: &mut impl ElfObject) -> Result<ElfHeader> {
        self.buf.prepare_ehdr(object)
    }

    pub fn read_phdr(
        &mut self,
        object: &mut impl ElfObject,
        ehdr: &ElfHeader,
    ) -> Result<&[ElfPhdr]> {
        self.buf.prepare_phdr(ehdr, object)
    }

    pub(crate) fn load_impl(
        &mut self,
        ehdr: ElfHeader,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
    ) -> Result<(Builder, &[ElfPhdr])> {
        let init_param = self.init_param;
        let phdrs = self.buf.prepare_phdr(&ehdr, &mut object)?;
        // 创建加载动态库所需的空间，并同时映射min_vaddr对应的segment
        let (param, min_vaddr) = create_segments(phdrs, ehdr.is_dylib());
        let memory = mmap_segment::<M>(&param, &mut object)?;
        let segments = ElfSegments {
            memory,
            offset: min_vaddr,
            len: param.len,
            munmap: M::munmap,
        };
        let mut builder = Builder::new(
            segments,
            object.file_name().to_owned(),
            lazy_bind,
            ehdr,
            init_param,
        );
        // 根据Phdr的类型进行不同操作
        for phdr in phdrs {
            if let Some(hook) = &self.hook {
                builder.exec_hook(hook, phdr)?;
            }
            match phdr.p_type {
                // 将segment加载到内存中
                PT_LOAD => {
                    if let Some(param) = load_segment(&builder.segments, phdr) {
                        mmap_segment::<M>(&param, &mut object)?;
                        fill_bss::<M>(&mut builder.segments, phdr)?;
                    }
                }
                _ => builder.parse_other_phdr::<M>(phdr),
            }
        }
        Ok((builder, phdrs))
    }

    pub(crate) async fn load_async_impl(
        &mut self,
        ehdr: ElfHeader,
        mut object: impl ElfObjectAsync,
        lazy_bind: Option<bool>,
    ) -> Result<(Builder, &[ElfPhdr])> {
        let init_param = self.init_param;
        let phdrs = self.buf.prepare_phdr(&ehdr, &mut object)?;
        // 创建加载动态库所需的空间，并同时映射min_vaddr对应的segment
        let (param, min_vaddr) = create_segments(phdrs, ehdr.is_dylib());
        let memory = mmap_segment_async::<M>(&param, &mut object).await?;
        let segments = ElfSegments {
            memory,
            offset: min_vaddr,
            len: param.len,
            munmap: M::munmap,
        };
        let mut builder = Builder::new(
            segments,
            object.file_name().to_owned(),
            lazy_bind,
            ehdr,
            init_param,
        );
        // 根据Phdr的类型进行不同操作
        for phdr in phdrs {
            if let Some(hook) = self.hook.as_ref() {
                builder.exec_hook(hook, phdr)?;
            }
            match phdr.p_type {
                // 将segment加载到内存中
                PT_LOAD => {
                    if let Some(param) = load_segment(&builder.segments, phdr) {
                        mmap_segment_async::<M>(&param, &mut object).await?;
                        fill_bss::<M>(&mut builder.segments, phdr)?;
                    }
                }
                _ => builder.parse_other_phdr::<M>(phdr),
            }
        }
        Ok((builder, phdrs))
    }
}
