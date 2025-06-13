use crate::{
    ElfObject, Result, UserData,
    arch::{Dyn, E_CLASS, EHDR_SIZE, EM_ARCH, Ehdr, ElfPhdr, Phdr},
    mmap::Mmap,
    object::ElfObjectAsync,
    parse_ehdr_error, parse_phdr_error,
    segment::{ELFRelro, ElfSegments},
};
use alloc::{borrow::ToOwned, boxed::Box, ffi::CString, format, sync::Arc, vec::Vec};
use core::{any::Any, ffi::CStr, marker::PhantomData, ops::Deref, ptr::NonNull};
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

pub(crate) struct Builder {
    pub(crate) phdr_mmap: Option<&'static [ElfPhdr]>,
    pub(crate) name: CString,
    pub(crate) lazy_bind: Option<bool>,
    pub(crate) ehdr: ElfHeader,
    pub(crate) relro: Option<ELFRelro>,
    pub(crate) dynamic_ptr: Option<NonNull<Dyn>>,
    pub(crate) user_data: UserData,
    pub(crate) segments: ElfSegments,
    pub(crate) init_fn: FnHandler,
    pub(crate) fini_fn: FnHandler,
    pub(crate) interp: Option<&'static str>,
}

impl Builder {
    const fn new(
        segments: ElfSegments,
        name: CString,
        lazy_bind: Option<bool>,
        ehdr: ElfHeader,
        init_fn: FnHandler,
        fini_fn: FnHandler,
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
            init_fn,
            fini_fn,
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
    dyn Fn(
        &CStr,
        &ElfPhdr,
        &ElfSegments,
        &mut UserData,
    ) -> core::result::Result<(), Box<dyn Any + Send + Sync>>,
>;

pub(crate) type FnHandler = Arc<dyn Fn(Option<fn()>, Option<&[fn()]>)>;

/// The elf object loader
pub struct Loader<M>
where
    M: Mmap,
{
    pub(crate) buf: ElfBuf,
    init_fn: FnHandler,
    fini_fn: FnHandler,
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
        let c_abi = Arc::new(|func: Option<fn()>, func_array: Option<&[fn()]>| {
            func.iter()
                .chain(func_array.unwrap_or(&[]).iter())
                .for_each(|init| unsafe { core::mem::transmute::<_, &extern "C" fn()>(init) }());
        });
        Self {
            hook: None,
            init_fn: c_abi.clone(),
            fini_fn: c_abi,
            buf: ElfBuf::new(),
            _marker: PhantomData,
        }
    }

    /// glibc passes argc, argv, and envp to functions in .init_array, as a non-standard extension.
    pub fn set_init(&mut self, init_fn: FnHandler) -> &mut Self {
        self.init_fn = init_fn;
        self
    }

    pub fn set_fini(&mut self, fini_fn: FnHandler) -> &mut Self {
        self.fini_fn = fini_fn;
        self
    }

    /// `hook` functions are called first when a program header is processed
    pub fn set_hook(&mut self, hook: Hook) -> &mut Self {
        self.hook = Some(hook);
        self
    }

    /// Read the elf header
    pub fn read_ehdr(&mut self, object: &mut impl ElfObject) -> Result<ElfHeader> {
        self.buf.prepare_ehdr(object)
    }

    /// Read the program header table
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
        let init_fn = self.init_fn.clone();
        let fini_fn = self.fini_fn.clone();
        let phdrs = self.buf.prepare_phdr(&ehdr, &mut object)?;
        // 创建加载动态库所需的空间，并同时映射min_vaddr对应的segment
        let segments = ElfSegments::create_segments::<M>(&mut object, phdrs, ehdr.is_dylib())?;
        let mut builder = Builder::new(
            segments,
            object.file_name().to_owned(),
            lazy_bind,
            ehdr,
            init_fn,
            fini_fn,
        );
        // 根据Phdr的类型进行不同操作
        for phdr in phdrs {
            if let Some(hook) = &self.hook {
                builder.exec_hook(hook, phdr)?;
            }
            match phdr.p_type {
                // 将segment加载到内存中
                PT_LOAD => builder.segments.load_segment::<M>(&mut object, phdr)?,
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
        let init_fn = self.init_fn.clone();
        let fini_fn = self.fini_fn.clone();
        let phdrs = self.buf.prepare_phdr(&ehdr, &mut object)?;
        // 创建加载动态库所需的空间，并同时映射min_vaddr对应的segment
        let segments =
            ElfSegments::create_segments_async::<M>(&mut object, phdrs, ehdr.is_dylib()).await?;
        let mut builder = Builder::new(
            segments,
            object.file_name().to_owned(),
            lazy_bind,
            ehdr,
            init_fn,
            fini_fn,
        );
        // 根据Phdr的类型进行不同操作
        for phdr in phdrs {
            if let Some(hook) = self.hook.as_ref() {
                builder.exec_hook(hook, phdr)?;
            }
            match phdr.p_type {
                // 将segment加载到内存中
                PT_LOAD => {
                    builder
                        .segments
                        .load_segment_async::<M>(&mut object, phdr)
                        .await?;
                }
                _ => builder.parse_other_phdr::<M>(phdr),
            }
        }
        Ok((builder, phdrs))
    }
}
