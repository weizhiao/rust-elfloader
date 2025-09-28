use crate::{
    ElfObject, Result, UserData,
    arch::{Dyn, EHDR_SIZE, ElfPhdr, ElfShdr, Phdr},
    ehdr::ElfHeader,
    mmap::Mmap,
    object::ElfObjectAsync,
    parse_phdr_error,
    segment::{ELFRelro, ElfSegments, SegmentBuilder, phdr::PhdrSegments, shdr::ShdrSegments},
    symbol::SymbolTable,
};
use alloc::{borrow::ToOwned, boxed::Box, ffi::CString, format, vec::Vec};
use core::{
    any::Any,
    ffi::{CStr, c_char},
    marker::PhantomData,
    ptr::NonNull,
};
use elf::abi::{PT_DYNAMIC, PT_GNU_RELRO, PT_INTERP, PT_PHDR, SHT_REL, SHT_SYMTAB};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

pub(crate) struct RelocatedBuilder {
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
    pub(crate) interp: Option<NonNull<c_char>>,
}

impl RelocatedBuilder {
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
                self.interp =
                    Some(NonNull::new(self.segments.get_mut_ptr(phdr.p_vaddr as usize)).unwrap());
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

    pub(crate) fn prepare_phdrs(
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

    pub(crate) fn prepare_shdrs_mut(
        &mut self,
        ehdr: &ElfHeader,
        object: &mut impl ElfObject,
    ) -> Result<&mut [ElfShdr]> {
        let (shdr_start, shdr_end) = ehdr.shdr_range();
        let size = shdr_end - shdr_start;
        if size > self.buf.len() {
            self.buf.resize(size, 0);
        }
        object.read(&mut self.buf[..size], shdr_start)?;
        unsafe {
            Ok(core::slice::from_raw_parts_mut(
                self.buf.as_mut_ptr().cast::<ElfShdr>(),
                self.buf.len() / size_of::<ElfShdr>(),
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
        self.buf.prepare_phdrs(ehdr, object)
    }

    pub(crate) fn load_relocated(
        &mut self,
        ehdr: ElfHeader,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
    ) -> Result<(RelocatedBuilder, &[ElfPhdr])> {
        let init_fn = self.init_fn.clone();
        let fini_fn = self.fini_fn.clone();
        let phdrs = self.buf.prepare_phdrs(&ehdr, &mut object)?;
        let mut phdr_segments = PhdrSegments::new(phdrs, ehdr.is_dylib(), object.as_fd().is_some());
        let segments = phdr_segments.load_segments::<M>(&mut object)?;
        phdr_segments.mprotect::<M>()?;
        let mut builder = RelocatedBuilder::new(
            segments,
            object.file_name().to_owned(),
            lazy_bind,
            ehdr,
            init_fn,
            fini_fn,
        );
        for phdr in phdrs {
            if let Some(hook) = &self.hook {
                builder.exec_hook(hook, phdr)?;
            }
            match phdr.p_type {
                _ => builder.parse_other_phdr::<M>(phdr),
            }
        }
        Ok((builder, phdrs))
    }

    // pub(crate) async fn load_async_impl(
    //     &mut self,
    //     ehdr: ElfHeader,
    //     mut object: impl ElfObjectAsync,
    //     lazy_bind: Option<bool>,
    // ) -> Result<(Builder, &[ElfPhdr])> {
    //     let init_fn = self.init_fn.clone();
    //     let fini_fn = self.fini_fn.clone();
    //     let phdrs = self.buf.prepare_phdrs(&ehdr, &mut object)?;
    //     // 创建加载动态库所需的空间，并同时映射min_vaddr对应的segment
    //     let segments =
    //         ElfSegments::create_segments::<M>(phdrs, ehdr.is_dylib(), object.as_fd().is_some())?;
    //     let mut builder = Builder::new(
    //         segments,
    //         object.file_name().to_owned(),
    //         lazy_bind,
    //         ehdr,
    //         init_fn,
    //         fini_fn,
    //     );
    //     // 根据Phdr的类型进行不同操作
    //     let mut last_addr = builder.segments.memory.as_ptr() as usize;
    //     for phdr in phdrs {
    //         if let Some(hook) = &self.hook {
    //             builder.exec_hook(hook, phdr)?;
    //         }
    //         match phdr.p_type {
    //             // 将segment加载到内存中
    //             PT_LOAD => {
    //                 builder
    //                     .segments
    //                     .load_segment_async::<M>(&mut object, phdr, &mut last_addr)
    //                     .await?
    //             }
    //             _ => builder.parse_other_phdr::<M>(phdr),
    //         }
    //     }
    //     Ok((builder, phdrs))
    // }

    pub(crate) fn load_rel(&mut self, ehdr: ElfHeader, mut object: impl ElfObject) -> Result<()> {
        let shdrs = self.buf.prepare_shdrs_mut(&ehdr, &mut object).unwrap();
        let mut shdr_segments = ShdrSegments::new(shdrs);
        let segments = shdr_segments.load_segments::<M>(&mut object)?;
        let base = segments.base();
        shdrs
            .iter_mut()
            .for_each(|shdr| shdr.sh_addr = (shdr.sh_addr as usize + base) as _);
        let mut relocations = Vec::new();
        let mut symtab = None;
        for shdr in shdrs.iter() {
            match shdr.sh_type {
                SHT_REL => relocations.push(shdr),
                SHT_SYMTAB => {
                    symtab = Some(SymbolTable::from_shdrs(&shdr, shdrs));
                }
                _ => {}
            }
        }
        Ok(())
    }
}
