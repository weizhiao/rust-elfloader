use crate::{
    arch::{Phdr, EHDR_SIZE, EM_ARCH, E_CLASS, PHDR_SIZE},
    dynamic::ElfRawDynamic,
    mmap::{self, Mmap, MmapImpl},
    parse_dynamic_error, parse_ehdr_error,
    relocation::ElfRelocation,
    segment::{ELFRelro, ElfSegments, MASK, PAGE_SIZE},
    symbol::SymbolData,
    ElfDylib, ElfObject, Result, ThreadLocal, Unwind, UserData,
};
use alloc::vec::Vec;
use core::{
    marker::PhantomData,
    mem::{forget, MaybeUninit},
};
use elf::{
    abi::{EI_NIDENT, ET_DYN, PT_DYNAMIC, PT_GNU_EH_FRAME, PT_GNU_RELRO, PT_LOAD, PT_PHDR},
    endian::NativeEndian,
    file::{parse_ident, FileHeader},
};

pub struct ELFEhdr {
    pub ehdr: FileHeader<NativeEndian>,
}

impl ELFEhdr {
    pub(crate) fn new(data: &[u8]) -> Result<ELFEhdr> {
        let ident_buf = &data[..EI_NIDENT];
        let tail_buf = &data[EI_NIDENT..EHDR_SIZE];
        let ident = parse_ident::<NativeEndian>(&ident_buf).map_err(parse_ehdr_error)?;
        let ehdr = FileHeader::parse_tail(ident, &tail_buf).map_err(parse_ehdr_error)?;
        Ok(ELFEhdr { ehdr })
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

    pub(crate) fn e_phnum(&self) -> usize {
        self.ehdr.e_phnum as usize
    }

    pub(crate) fn e_phentsize(&self) -> usize {
        self.ehdr.e_phentsize as usize
    }

    pub(crate) fn e_phoff(&self) -> usize {
        self.ehdr.e_phoff as usize
    }

    pub(crate) fn phdr_range(&self) -> (usize, usize) {
        let phdrs_size = self.e_phentsize() * self.e_phnum();
        let phdr_start = self.e_phoff();
        let phdr_end = phdr_start + phdrs_size;
        (phdr_start, phdr_end)
    }
}

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
    pub fn new(object: O) -> Self {
        Self {
            object,
            _marker: PhantomData,
        }
    }

    /// Load and validate ehdr
    pub fn load_ehdr(&mut self) -> Result<ELFEhdr> {
        let mut buf: MaybeUninit<[u8; EHDR_SIZE]> = MaybeUninit::uninit();
        self.object.read(unsafe { &mut *buf.as_mut_ptr() }, 0)?;
        let buf = unsafe { buf.assume_init() };
        let ehdr = ELFEhdr::new(&buf)?;
        ehdr.validate()?;
        Ok(ehdr)
    }

    /// Parse ehdr to get phdrs
    pub fn parse_ehdr(&mut self, ehdr: ELFEhdr) -> crate::Result<Vec<Phdr>> {
        let (phdr_start, phdr_end) = ehdr.phdr_range();
        let phdrs_size = phdr_end - phdr_start;
        let phdr_num = phdrs_size / PHDR_SIZE;
        let mut phdr_buf = Vec::with_capacity(phdrs_size);
        unsafe { phdr_buf.set_len(phdrs_size) };
        self.object.read(&mut phdr_buf, phdr_start)?;
        let phdrs =
            unsafe { Vec::from_raw_parts(phdr_buf.as_mut_ptr() as *mut Phdr, phdr_num, phdr_num) };
        forget(phdr_buf);
        Ok(phdrs)
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
            M::mmap(
                None,
                total_size,
                ElfSegments::map_prot(min_prot),
                mmap::MapFlags::MAP_PRIVATE,
                self.object.transport(min_off, min_filesz),
            )?
        };
        Ok(ElfSegments::new(
            memory,
            -(min_vaddr as isize),
            total_size,
            M::munmap,
        ))
    }

    fn load_segment(&self, segments: &ElfSegments, phdr: &Phdr) -> crate::Result<()> {
        // 映射的起始地址与结束地址都是页对齐的
        let addr_min = (-segments.offset()) as usize;
        let base = segments.base();
        let min_vaddr = phdr.p_vaddr as usize & MASK;
        let max_vaddr = (phdr.p_vaddr as usize + phdr.p_memsz as usize + PAGE_SIZE - 1) & MASK;
        let memsz = max_vaddr - min_vaddr;
        let prot = ElfSegments::map_prot(phdr.p_flags);
        let real_addr = min_vaddr + base;
        let offset = phdr.p_offset as usize;
        let filesz = phdr.p_filesz as usize;
        // 将类似bss节的内存区域的值设置为0
        if addr_min != min_vaddr {
            let _ = unsafe {
                M::mmap(
                    Some(real_addr),
                    memsz,
                    prot,
                    mmap::MapFlags::MAP_PRIVATE | mmap::MapFlags::MAP_FIXED,
                    self.object.transport(offset, filesz),
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
                            mmap::MapFlags::MAP_PRIVATE | mmap::MapFlags::MAP_FIXED,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Load a dynamic library into memory
    pub fn load_dylib<T, U>(mut self) -> Result<ElfDylib<T, U>>
    where
        T: ThreadLocal,
        U: Unwind,
    {
        let ehdr = self.load_ehdr()?;
        let (phdr_start, phdr_end) = ehdr.phdr_range();
        let entry = ehdr.ehdr.e_entry;
        let phdrs = self.parse_ehdr(ehdr)?;
        // 创建加载动态库所需的空间，并同时映射min_vaddr对应的segment
        let segments = self.create_segments(&phdrs)?;
        // 获取基地址
        let base = segments.base();
        let mut unwind = None;
        let mut dynamics = None;
        let mut relro = None;
        let mut phdr_mmap = None;
        #[cfg(feature = "tls")]
        let mut tls = None;

        // 根据Phdr的类型进行不同操作
        for phdr in phdrs.iter() {
            match phdr.p_type {
                // 将segment加载到内存中
                PT_LOAD => self.load_segment(&segments, phdr)?,
                // 解析.dynamic section
                PT_DYNAMIC => {
                    dynamics = Some(ElfRawDynamic::new((phdr.p_vaddr as usize + base) as _)?)
                }
                PT_GNU_EH_FRAME => {
                    unwind =
                        unsafe { U::new(phdr, segments.base()..segments.base() + segments.len()) }
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
                #[cfg(feature = "tls")]
                elf::abi::PT_TLS => tls = unsafe { T::new(phdr, base) },
                _ => {}
            }
        }

        let phdrs = phdr_mmap.unwrap_or_else(|| {
            for phdr in phdrs {
                let cur_range = phdr.p_offset as usize..(phdr.p_offset + phdr.p_filesz) as usize;
                if cur_range.contains(&phdr_start) && cur_range.contains(&phdr_end) {
                    return unsafe {
                        core::slice::from_raw_parts(
                            (segments.base() + phdr_start - cur_range.start) as *const Phdr,
                            (cur_range.end - cur_range.start) / size_of::<Phdr>(),
                        )
                    };
                }
            }
            unreachable!()
        });

        let dynamics = dynamics
            .ok_or(parse_dynamic_error("elf file does not have dynamic"))?
            .finish(base);
        let relocation = ElfRelocation::new(dynamics.pltrel, dynamics.dynrel);
        let symbols = SymbolData::new(&dynamics);
        let needed_libs: Vec<&'static str> = dynamics
            .needed_libs
            .iter()
            .map(|needed_lib| symbols.strtab().get(*needed_lib))
            .collect();
        let user_data = UserData::empty();
        let name = self.object.file_name();
        let elf_lib = ElfDylib {
            name,
            entry: entry as usize,
            phdrs,
            symbols,
            dynamic: dynamics.dyn_ptr,
            #[cfg(feature = "tls")]
            tls,
            unwind,
            segments,
            fini_fn: dynamics.fini_fn,
            fini_array_fn: dynamics.fini_array_fn,
            user_data,
            dep_libs: Vec::new(),
            relro,
            relocation,
            init_fn: dynamics.init_fn,
            init_array_fn: dynamics.init_array_fn,
            needed_libs,
            _marker: PhantomData,
        };
        Ok(elf_lib)
    }
}
