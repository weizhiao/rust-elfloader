use crate::{
    arch::{Phdr, EHDR_SIZE, EM_ARCH, E_CLASS, PHDR_SIZE},
    dynamic::ElfRawDynamic,
    mmap::{self, Mmap, MmapImpl, MmapRange},
    parse_dynamic_error, parse_ehdr_error,
    relocation::ElfRelocation,
    segment::{ELFRelro, ElfSegments, MASK, PAGE_SIZE},
    symbol::SymbolTable,
    CoreComponent, CoreComponentInner, ElfDylib, ElfObject, Result, UserData,
};
use alloc::{borrow::ToOwned, sync::Arc, vec::Vec};
use core::{ffi::CStr, marker::PhantomData, mem::MaybeUninit, ptr::null};
use elf::{
    abi::{EI_NIDENT, ET_DYN, PT_DYNAMIC, PT_GNU_RELRO, PT_LOAD, PT_PHDR},
    endian::NativeEndian,
    file::{parse_ident, FileHeader},
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

/// The elf object loader
pub struct Loader<O, M = MmapImpl>
where
    O: ElfObject,
    M: Mmap,
{
    object: O,
    _marker: PhantomData<M>,
}

impl<O: ElfObject, M: Mmap> Loader<O, M> {
    /// Create a new loader
    pub const fn new(object: O) -> Self {
        Self {
            object,
            _marker: PhantomData,
        }
    }

    fn create_segments(&mut self, phdrs: &[Phdr]) -> crate::Result<ElfSegments> {
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
        let memory = unsafe {
            M::mmap_segment(
                None,
                total_size,
                ElfSegments::map_prot(min_prot),
                mmap::MapFlags::MAP_PRIVATE,
                MmapRange {
                    offset: min_off,
                    len: min_filesz,
                },
                &mut self.object,
            )?
        };
        Ok(ElfSegments {
            memory,
            offset: min_vaddr,
            len: total_size,
            munmap: M::munmap,
        })
    }

    fn load_segment(&mut self, segments: &ElfSegments, phdr: &Phdr) -> crate::Result<()> {
        // 映射的起始地址与结束地址都是页对齐的
        let addr_min = segments.offset();
        let base = segments.base();
        let min_vaddr = phdr.p_vaddr as usize & MASK;
        let max_vaddr = (phdr.p_vaddr as usize + phdr.p_memsz as usize + PAGE_SIZE - 1) & MASK;
        let memsz = max_vaddr - min_vaddr;
        let prot = ElfSegments::map_prot(phdr.p_flags);
        let real_addr = min_vaddr + base;
        let offset = phdr.p_offset as usize & MASK;
        let align_len = phdr.p_offset as usize - offset;
        let filesz = phdr.p_filesz as usize + align_len;
        // 将类似bss节的内存区域的值设置为0
        if addr_min != min_vaddr {
            let _ = unsafe {
                M::mmap_segment(
                    Some(real_addr),
                    memsz,
                    prot,
                    mmap::MapFlags::MAP_PRIVATE | mmap::MapFlags::MAP_FIXED,
                    MmapRange {
                        len: filesz,
                        offset,
                    },
                    &mut self.object,
                )?
            };
            //将类似bss节的内存区域的值设置为0
            if phdr.p_filesz != phdr.p_memsz {
                // 用0填充这一页
                let zero_start = (phdr.p_vaddr + phdr.p_filesz) as usize;
                let zero_end = (zero_start + PAGE_SIZE - 1) & MASK;
                let zero_mem = &mut segments.as_mut_slice()[zero_start..zero_end];
                zero_mem.fill(0);

                if zero_end < max_vaddr {
                    //之后剩余的一定是页的整数倍
                    //如果有剩余的页的话，将其映射为匿名页
                    let zero_mmap_addr = base + zero_end;
                    let zero_mmap_len = max_vaddr - zero_end;
                    unsafe {
                        M::mmap_anonymous(
                            zero_mmap_addr,
                            zero_mmap_len,
                            prot,
                            mmap::MapFlags::MAP_PRIVATE
                                | mmap::MapFlags::MAP_FIXED
                                | mmap::MapFlags::MAP_ANONYMOUS,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Load a dynamic library into memory
    /// # Note
    /// `hook` functions are called first when a program header is processed.
    pub fn load_dylib<F>(mut self, lazy_bind: Option<bool>, hook: F) -> Result<ElfDylib>
    where
        F: Fn(&CStr, &Phdr, &ElfSegments, &mut UserData) -> Result<()>,
    {
        const MAX_BUF_SIZE: usize = EHDR_SIZE + 12 * PHDR_SIZE;
        let mut stack_buf: MaybeUninit<[u8; MAX_BUF_SIZE]> = MaybeUninit::uninit();
        let stack_buf_ref = unsafe { &mut *stack_buf.as_mut_ptr() };
        self.object.read(stack_buf_ref, 0)?;
        let ehdr = ElfHeader::new(stack_buf_ref)?;
        ehdr.validate()?;
        let (phdr_start, phdr_end) = ehdr.phdr_range();
        let phdr_num = ehdr.e_phnum();
        let mut heap_buf = Vec::new();
        let phdrs = if MAX_BUF_SIZE >= phdr_end {
            unsafe {
                core::slice::from_raw_parts(
                    stack_buf
                        .as_ptr()
                        .cast::<u8>()
                        .add(phdr_start)
                        .cast::<Phdr>(),
                    phdr_num,
                )
            }
        } else {
            heap_buf.resize(phdr_end - phdr_start, 0);
            self.object.read(&mut heap_buf, phdr_start)?;
            unsafe { core::slice::from_raw_parts(heap_buf.as_ptr().cast::<Phdr>(), phdr_num) }
        };
        // 创建加载动态库所需的空间，并同时映射min_vaddr对应的segment
        let segments = self.create_segments(&phdrs)?;
        // 获取基地址
        let base = segments.base();
        let mut dynamic = None;
        let mut relro = None;
        let mut phdr_mmap = None;
        let mut user_data = UserData::empty();
        let name = self.object.file_name().to_owned();

        // 根据Phdr的类型进行不同操作
        for phdr in phdrs.iter() {
            hook(&name, phdr, &segments, &mut user_data)?;
            match phdr.p_type {
                // 将segment加载到内存中
                PT_LOAD => self.load_segment(&segments, phdr)?,
                // 解析.dynamic section
                PT_DYNAMIC => {
                    dynamic = Some(ElfRawDynamic::new((phdr.p_vaddr as usize + base) as _)?)
                }
                PT_GNU_RELRO => relro = Some(ELFRelro::new::<M>(phdr, segments.base())),
                PT_PHDR => {
                    phdr_mmap = Some(unsafe {
                        core::slice::from_raw_parts(
                            (segments.base() + phdr.p_vaddr as usize) as *const Phdr,
                            phdr.p_memsz as usize / size_of::<Phdr>(),
                        )
                    })
                }
                _ => {}
            }
        }
        // 获取映射到内存中的Phdr
        let phdrs = phdr_mmap.unwrap_or_else(|| {
            for phdr in phdrs {
                let cur_range = phdr.p_offset as usize..(phdr.p_offset + phdr.p_filesz) as usize;
                if cur_range.contains(&phdr_start) && cur_range.contains(&phdr_end) {
                    return unsafe {
                        core::slice::from_raw_parts(
                            (base + phdr_start - cur_range.start) as *const Phdr,
                            (cur_range.end - cur_range.start) / size_of::<Phdr>(),
                        )
                    };
                }
            }
            unreachable!()
        });
        let dynamic = dynamic
            .ok_or(parse_dynamic_error("elf file does not have dynamic"))?
            .finish(segments.base());
        let relocation = ElfRelocation::new(dynamic.pltrel, dynamic.dynrel, dynamic.rela_count);
        let symbols = SymbolTable::new(&dynamic);
        let needed_libs: Vec<&'static str> = dynamic
            .needed_libs
            .iter()
            .map(|needed_lib| unsafe { symbols.strtab().get(needed_lib.get()) })
            .collect();

        let elf_lib = ElfDylib {
            entry: ehdr.ehdr.e_entry as usize,
            relro,
            relocation,
            init_fn: dynamic.init_fn,
            init_array_fn: dynamic.init_array_fn,
            lazy: lazy_bind.unwrap_or(!dynamic.bind_now),
            got: dynamic.got,
            rpath: dynamic
                .rpath_off
                .map(|rpath_off| unsafe { symbols.strtab().get(rpath_off.get()) }),
            runpath: dynamic
                .runpath_off
                .map(|runpath_off| unsafe { symbols.strtab().get(runpath_off.get()) }),
            core: CoreComponent {
                inner: Arc::new(CoreComponentInner {
                    name,
                    symbols,
                    dynamic: dynamic.dyn_ptr,
                    pltrel: dynamic.pltrel.map_or(null(), |plt| plt.as_ptr()),
                    phdrs,
                    fini_fn: dynamic.fini_fn,
                    fini_array_fn: dynamic.fini_array_fn,
                    segments,
                    needed_libs: needed_libs.into_boxed_slice(),
                    user_data,
                    lazy_scope: None,
                }),
            },
        };
        Ok(elf_lib)
    }
}
