use crate::{
    CoreComponent, CoreComponentInner, ElfDylib, ElfObject, InitParams, Result, UserData,
    arch::{E_CLASS, EHDR_SIZE, EM_ARCH, PHDR_SIZE, Phdr},
    dynamic::ElfRawDynamic,
    mmap::{self, MapFlags, Mmap, ProtFlags},
    object::ElfObjectAsync,
    parse_dynamic_error, parse_ehdr_error, parse_phdr_error,
    relocation::ElfRelocation,
    segment::{ELFRelro, ElfSegments, MASK, PAGE_SIZE},
    symbol::SymbolTable,
};
use alloc::{borrow::ToOwned, boxed::Box, ffi::CString, format, sync::Arc, vec::Vec};
use core::{
    any::Any,
    ffi::{CStr, c_void},
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::{NonNull, null},
    sync::atomic::AtomicBool,
};
use elf::{
    abi::{EI_NIDENT, ET_DYN, PT_DYNAMIC, PT_GNU_RELRO, PT_LOAD, PT_PHDR},
    endian::NativeEndian,
    file::{FileHeader, parse_ident},
};

pub struct ElfHeader {
    pub ehdr: FileHeader<NativeEndian>,
}

impl ElfHeader {
    pub(crate) fn new(data: &[u8]) -> Result<ElfHeader> {
        let ident_buf = &data[..EI_NIDENT];
        let tail_buf = &data[EI_NIDENT..EHDR_SIZE];
        let ident = parse_ident::<NativeEndian>(&ident_buf).map_err(parse_ehdr_error)?;
        let ehdr = FileHeader::parse_tail(ident, &tail_buf).map_err(parse_ehdr_error)?;
        Ok(ElfHeader { ehdr })
    }

    //验证elf头
    #[inline]
    pub(crate) fn validate(&self) -> Result<()> {
        if self.ehdr.e_type != ET_DYN {
            return Err(parse_ehdr_error("file type mismatch"));
        }

        if self.ehdr.e_machine != EM_ARCH {
            return Err(parse_ehdr_error("file arch mismatch"));
        }

        if self.ehdr.class != E_CLASS {
            return Err(parse_ehdr_error("file class mismatch"));
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
        let dest =
            unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr().cast::<u8>(), param.range.len) };
        object.read(dest, param.range.offset)?;
        unsafe { M::mprotect(ptr, param.len, param.prot) }?;
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
        object.read(dest, param.range.offset).await?;
        unsafe { M::mprotect(ptr, param.len, param.prot) }?;
    }
    Ok(ptr)
}

#[inline]
fn create_segments(phdrs: &[Phdr]) -> (MmapParam, usize) {
    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0;
    // 最小偏移地址对应内容在文件中的偏移
    let mut min_off = 0;
    let mut min_filesz = 0;
    let mut min_prot = 0;

    //找到最小的偏移地址和最大的偏移地址
    for phdr in phdrs.iter() {
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
    min_vaddr &= MASK as usize;
    let total_size = max_vaddr - min_vaddr;
    let prot = ElfSegments::map_prot(min_prot);
    (
        MmapParam {
            addr: None,
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
fn fill_bss<M: Mmap>(segments: &ElfSegments, phdr: &Phdr) -> Result<()> {
    if phdr.p_filesz != phdr.p_memsz {
        let prot = ElfSegments::map_prot(phdr.p_flags);
        let max_vaddr = (phdr.p_vaddr as usize + phdr.p_memsz as usize + PAGE_SIZE - 1) & MASK;
        // 用0填充这一页
        let zero_start = (phdr.p_vaddr + phdr.p_filesz) as usize;
        let zero_end = (zero_start + PAGE_SIZE - 1) & MASK;
        let zero_mem = &mut segments.as_mut_slice()[zero_start..zero_end];
        zero_mem.fill(0);

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

struct TempData {
    phdr_mmap: Option<&'static [Phdr]>,
    name: CString,
    lazy_bind: Option<bool>,
    ehdr: ElfHeader,
    relro: Option<ELFRelro>,
    dynamic: Option<ElfRawDynamic>,
    user_data: UserData,
    segments: ElfSegments,
    init_params: Option<InitParams>,
}

impl TempData {
    const fn new(
        segments: ElfSegments,
        name: CString,
        lazy_bind: Option<bool>,
        ehdr: ElfHeader,
        init_params: Option<InitParams>,
    ) -> Self {
        Self {
            phdr_mmap: None,
            name,
            lazy_bind,
            ehdr,
            relro: None,
            dynamic: None,
            segments,
            user_data: UserData::empty(),
            init_params,
        }
    }

    fn exec_hook<F>(&mut self, hook: &F, phdr: &Phdr) -> Result<()>
    where
        F: Fn(&CStr, &Phdr, &ElfSegments, &mut UserData) -> core::result::Result<(), Box<dyn Any>>,
    {
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

    fn parse_other_phdr<M: Mmap>(&mut self, phdr: &Phdr) -> Result<()> {
        match phdr.p_type {
            // 解析.dynamic section
            PT_DYNAMIC => {
                self.dynamic = Some(ElfRawDynamic::new(
                    (phdr.p_vaddr as usize + self.segments.base()) as _,
                )?)
            }
            PT_GNU_RELRO => self.relro = Some(ELFRelro::new::<M>(phdr, self.segments.base())),
            PT_PHDR => {
                self.phdr_mmap = Some(unsafe {
                    core::slice::from_raw_parts(
                        (self.segments.base() + phdr.p_vaddr as usize) as *const Phdr,
                        phdr.p_memsz as usize / size_of::<Phdr>(),
                    )
                })
            }
            _ => {}
        };
        Ok(())
    }

    fn create_dylib(self, phdrs: &[Phdr]) -> Result<ElfDylib> {
        let (phdr_start, phdr_end) = self.ehdr.phdr_range();
        // 获取映射到内存中的Phdr
        let phdrs = self.phdr_mmap.unwrap_or_else(|| {
            for phdr in phdrs {
                let cur_range = phdr.p_offset as usize..(phdr.p_offset + phdr.p_filesz) as usize;
                if cur_range.contains(&phdr_start) && cur_range.contains(&phdr_end) {
                    return unsafe {
                        core::slice::from_raw_parts(
                            (self.segments.base() + phdr_start - cur_range.start) as *const Phdr,
                            self.ehdr.e_phnum(),
                        )
                    };
                }
            }
            unreachable!()
        });
        let dynamic = self
            .dynamic
            .ok_or(parse_dynamic_error("elf file does not have dynamic"))?
            .finish(self.segments.base());
        let relocation = ElfRelocation::new(dynamic.pltrel, dynamic.dynrel, dynamic.rela_count);
        let symbols = SymbolTable::new(&dynamic);
        let needed_libs: Vec<&'static str> = dynamic
            .needed_libs
            .iter()
            .map(|needed_lib| symbols.strtab().get_str(needed_lib.get()))
            .collect();
        let elf_lib = ElfDylib {
            entry: self.ehdr.ehdr.e_entry as usize,
            relro: self.relro,
            relocation,
            init_params: self.init_params,
            init_fn: dynamic.init_fn,
            init_array_fn: dynamic.init_array_fn,
            lazy: self.lazy_bind.unwrap_or(!dynamic.bind_now),
            got: dynamic.got,
            rpath: dynamic
                .rpath_off
                .map(|rpath_off| symbols.strtab().get_str(rpath_off.get())),
            runpath: dynamic
                .runpath_off
                .map(|runpath_off| symbols.strtab().get_str(runpath_off.get())),
            core: CoreComponent {
                inner: Arc::new(CoreComponentInner {
                    is_init: AtomicBool::new(false),
                    name: self.name,
                    symbols,
                    dynamic: dynamic.dyn_ptr,
                    pltrel: dynamic.pltrel.map_or(null(), |plt| plt.as_ptr()),
                    phdrs,
                    fini_fn: dynamic.fini_fn,
                    fini_array_fn: dynamic.fini_array_fn,
                    segments: self.segments,
                    needed_libs: needed_libs.into_boxed_slice(),
                    user_data: self.user_data,
                    lazy_scope: None,
                }),
            },
        };
        Ok(elf_lib)
    }
}

struct ElfBuf {
    stack_buf: MaybeUninit<[u8; EHDR_SIZE + 12 * PHDR_SIZE]>,
    heap_buf: Vec<u8>,
}

impl ElfBuf {
    const MAX_BUF_SIZE: usize = EHDR_SIZE + 12 * PHDR_SIZE;

    const fn new() -> Self {
        ElfBuf {
            stack_buf: MaybeUninit::uninit(),
            heap_buf: Vec::new(),
        }
    }

    #[inline]
    fn stack_buf(&mut self) -> &mut [u8] {
        unsafe { &mut *self.stack_buf.as_mut_ptr() }
    }

    #[inline]
    fn get_phdrs_from_stack(&mut self, phdr_start: usize, phdr_end: usize) -> Option<&[Phdr]> {
        if Self::MAX_BUF_SIZE >= phdr_end {
            unsafe {
                Some(core::slice::from_raw_parts(
                    self.stack_buf
                        .as_ptr()
                        .cast::<u8>()
                        .add(phdr_start)
                        .cast::<Phdr>(),
                    (phdr_end - phdr_start) / size_of::<Phdr>(),
                ))
            }
        } else {
            self.heap_buf.resize(phdr_end - phdr_start, 0);
            None
        }
    }

    #[inline]
    fn heap_buf(&mut self) -> &mut Vec<u8> {
        &mut self.heap_buf
    }

    #[inline]
    fn get_phdrs_from_heap(&self) -> &[Phdr] {
        unsafe {
            core::slice::from_raw_parts(
                self.heap_buf.as_ptr().cast::<Phdr>(),
                self.heap_buf.len() / size_of::<Phdr>(),
            )
        }
    }
}

/// The elf object loader
pub struct Loader<M>
where
    M: Mmap,
{
    init_params: Option<InitParams>,
    _marker: PhantomData<M>,
}

impl<M: Mmap> Loader<M> {
    /// Create a new loader
    pub const fn new() -> Self {
        Self {
            init_params: None,
            _marker: PhantomData,
        }
    }

    /// glibc passes argc, argv, and envp to functions in .init_array, as a non-standard extension.
    pub fn set_init_params(&mut self, argc: usize, argv: usize, envp: usize) {
        self.init_params = Some(InitParams { argc, argv, envp });
    }

    /// Load a dynamic library into memory
    pub fn easy_load_dylib(&self, object: impl ElfObject) -> Result<ElfDylib> {
        self.load_dylib(object, None, |_, _, _, _| Ok(()))
    }

    /// Load a dynamic library into memory
    /// # Note
    /// * `hook` functions are called first when a program header is processed.
    /// * When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    pub fn load_dylib<F>(
        &self,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
        hook: F,
    ) -> Result<ElfDylib>
    where
        F: Fn(&CStr, &Phdr, &ElfSegments, &mut UserData) -> core::result::Result<(), Box<dyn Any>>,
    {
        let mut buf = ElfBuf::new();
        object.read(buf.stack_buf(), 0)?;
        let ehdr = ElfHeader::new(buf.stack_buf())?;
        ehdr.validate()?;
        let (phdr_start, phdr_end) = ehdr.phdr_range();
        let phdrs = if let Some(phdrs) = buf.get_phdrs_from_stack(phdr_start, phdr_end) {
            phdrs
        } else {
            object.read(buf.heap_buf(), phdr_start)?;
            buf.get_phdrs_from_heap()
        };
        // 创建加载动态库所需的空间，并同时映射min_vaddr对应的segment
        let (param, min_vaddr) = create_segments(&phdrs);
        let memory = mmap_segment::<M>(&param, &mut object)?;
        let segments = ElfSegments {
            memory,
            offset: min_vaddr,
            len: param.len,
            munmap: M::munmap,
        };
        let mut temp_data = TempData::new(
            segments,
            object.file_name().to_owned(),
            lazy_bind,
            ehdr,
            self.init_params,
        );
        // 根据Phdr的类型进行不同操作
        for phdr in phdrs.iter() {
            temp_data.exec_hook(&hook, phdr)?;
            match phdr.p_type {
                // 将segment加载到内存中
                PT_LOAD => {
                    if let Some(param) = load_segment(&temp_data.segments, phdr) {
                        mmap_segment::<M>(&param, &mut object)?;
                        fill_bss::<M>(&temp_data.segments, phdr)?;
                    }
                }
                _ => temp_data.parse_other_phdr::<M>(phdr)?,
            }
        }
        temp_data.create_dylib(phdrs)
    }

    /// Load a dynamic library into memory
    /// # Note
    /// `hook` functions are called first when a program header is processed.
    pub async fn load_dylib_async<F>(
        &self,
        mut object: impl ElfObjectAsync,
        lazy_bind: Option<bool>,
        hook: F,
    ) -> Result<ElfDylib>
    where
        F: Fn(&CStr, &Phdr, &ElfSegments, &mut UserData) -> core::result::Result<(), Box<dyn Any>>,
    {
        let mut buf = ElfBuf::new();
        object.read(buf.stack_buf(), 0).await?;
        let ehdr = ElfHeader::new(buf.stack_buf())?;
        ehdr.validate()?;
        let (phdr_start, phdr_end) = ehdr.phdr_range();
        let phdrs = if let Some(phdrs) = buf.get_phdrs_from_stack(phdr_start, phdr_end) {
            phdrs
        } else {
            object.read(buf.heap_buf(), phdr_start).await?;
            buf.get_phdrs_from_heap()
        };
        // 创建加载动态库所需的空间，并同时映射min_vaddr对应的segment
        let (param, min_vaddr) = create_segments(&phdrs);
        let memory = mmap_segment_async::<M>(&param, &mut object).await?;
        let segments = ElfSegments {
            memory,
            offset: min_vaddr,
            len: param.len,
            munmap: M::munmap,
        };
        let mut temp_data = TempData::new(
            segments,
            object.file_name().to_owned(),
            lazy_bind,
            ehdr,
            self.init_params,
        );

        // 根据Phdr的类型进行不同操作
        for phdr in phdrs.iter() {
            temp_data.exec_hook(&hook, phdr)?;
            match phdr.p_type {
                // 将segment加载到内存中
                PT_LOAD => {
                    if let Some(param) = load_segment(&temp_data.segments, phdr) {
                        mmap_segment_async::<M>(&param, &mut object).await?;
                        fill_bss::<M>(&temp_data.segments, phdr)?;
                    }
                }
                _ => temp_data.parse_other_phdr::<M>(phdr)?,
            }
        }
        temp_data.create_dylib(phdrs)
    }
}
