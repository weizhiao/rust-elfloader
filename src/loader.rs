use crate::{
    ElfObject, Result, UserData,
    arch::{EHDR_SIZE, ElfPhdr, ElfShdr},
    ehdr::ElfHeader,
    format::{
        relocatable::{ElfRelocatable, RelocatableBuilder},
        relocated::{RelocatedBuilder, RelocatedCommonPart},
    },
    mmap::Mmap,
    segment::{ElfSegments, SegmentBuilder, phdr::PhdrSegments, shdr::ShdrSegments},
};
use alloc::{borrow::ToOwned, boxed::Box, vec::Vec};
use core::{any::Any, ffi::CStr, marker::PhantomData};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

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
                (phdr_end - phdr_start) / size_of::<ElfPhdr>(),
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
                (shdr_end - shdr_start) / size_of::<ElfShdr>(),
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

    /// Set the finalization function handler
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

    /// Load a relocated ELF object
    pub(crate) fn load_relocated<'loader>(
        &'loader mut self,
        ehdr: ElfHeader,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
    ) -> Result<RelocatedCommonPart> {
        let init_fn = self.init_fn.clone();
        let fini_fn = self.fini_fn.clone();
        let phdrs = self.buf.prepare_phdrs(&ehdr, &mut object)?;
        let mut phdr_segments = PhdrSegments::new(phdrs, ehdr.is_dylib(), object.as_fd().is_some());
        let segments = phdr_segments.load_segments::<M>(&mut object)?;
        phdr_segments.mprotect::<M>()?;
        let builder: RelocatedBuilder<'_, M> = RelocatedBuilder::new(
            self.hook.as_ref(),
            segments,
            object.file_name().to_owned(),
            lazy_bind,
            ehdr,
            init_fn,
            fini_fn,
        );
        Ok(builder.build(phdrs)?)
    }

    /// Load a relocatable ELF object
    pub(crate) fn load_rel(
        &mut self,
        ehdr: ElfHeader,
        mut object: impl ElfObject,
    ) -> Result<ElfRelocatable> {
        let init_fn = self.init_fn.clone();
        let fini_fn = self.fini_fn.clone();
        let shdrs = self.buf.prepare_shdrs_mut(&ehdr, &mut object).unwrap();
        let mut shdr_segments = ShdrSegments::new(shdrs, &mut object);
        let segments = shdr_segments.load_segments::<M>(&mut object)?;
        let pltgot = shdr_segments.take_pltgot();
        let mprotect = Box::new(move || {
            shdr_segments.mprotect::<M>()?;
            Ok(())
        });
        let builder = RelocatableBuilder::new(
            object.file_name().to_owned(),
            shdrs,
            init_fn,
            fini_fn,
            segments,
            mprotect,
            pltgot,
        );
        Ok(builder.build())
    }
}
