use crate::{
    ElfReader, Result,
    arch::{EHDR_SIZE, ElfPhdr, ElfShdr},
    ehdr::ElfHeader,
    format::{DynamicImage, ImageBuilder, ObjectBuilder, ObjectImage, StaticImage},
    mmap::Mmap,
    os::DefaultMmap,
    segment::{ElfSegments, SegmentBuilder, phdr::PhdrSegments, shdr::ShdrSegments},
};
use alloc::{borrow::ToOwned, boxed::Box, vec::Vec};
use core::marker::PhantomData;

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

    pub(crate) fn prepare_ehdr(&mut self, object: &mut impl ElfReader) -> Result<ElfHeader> {
        object.read(&mut self.buf[..EHDR_SIZE], 0)?;
        ElfHeader::new(&self.buf).cloned()
    }

    pub(crate) fn prepare_phdrs(
        &mut self,
        ehdr: &ElfHeader,
        object: &mut impl ElfReader,
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
        object: &mut impl ElfReader,
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

/// Context provided to hook functions during ELF loading.
pub struct LoadHookContext<'a, D> {
    name: &'a str,
    phdr: &'a ElfPhdr,
    segments: &'a ElfSegments,
    user_data: &'a mut D,
}

impl<'a, D> LoadHookContext<'a, D> {
    pub(crate) fn new(
        name: &'a str,
        phdr: &'a ElfPhdr,
        segments: &'a ElfSegments,
        user_data: &'a mut D,
    ) -> Self {
        Self {
            name,
            phdr,
            segments,
            user_data,
        }
    }

    /// Returns the name of the ELF object being loaded.
    pub fn name(&self) -> &str {
        self.name
    }

    /// Returns the program header for the current segment.
    pub fn phdr(&self) -> &ElfPhdr {
        self.phdr
    }

    /// Returns the ELF segments.
    pub fn segments(&self) -> &ElfSegments {
        self.segments
    }

    /// Returns mutable access to the user-defined data.
    pub fn user_data_mut(&mut self) -> &mut D {
        self.user_data
    }
}

/// Hook trait for processing program headers during loading.
///
/// # Examples
/// ```rust
/// use elf_loader::{LoadHook, LoadHookContext, Result};
///
/// struct MyHook;
///
/// impl LoadHook<()> for MyHook {
///     fn call<'a>(&'a self, ctx: &'a mut LoadHookContext<'a, ()>) -> Result<()> {
///         println!("Processing segment: {:?}", ctx.phdr());
///         Ok(())
///     }
/// }
/// ```
pub trait LoadHook<D = ()> {
    /// Executes the hook with the provided context.
    fn call<'a>(&'a self, ctx: &'a mut LoadHookContext<'a, D>) -> Result<()>;
}

impl<F, D> LoadHook<D> for F
where
    F: for<'a> Fn(&'a mut LoadHookContext<'a, D>) -> Result<()>,
{
    fn call<'a>(&'a self, ctx: &'a mut LoadHookContext<'a, D>) -> Result<()> {
        (self)(ctx)
    }
}

impl LoadHook for () {
    fn call<'a>(&'a self, _ctx: &'a mut LoadHookContext<'a, ()>) -> Result<()> {
        Ok(())
    }
}

pub(crate) type FnHandler = Arc<dyn Fn(Option<fn()>, Option<&[fn()]>)>;

/// The ELF object loader.
///
/// `Loader` is responsible for orchestrating the loading of ELF objects into memory.
///
/// # Examples
/// ```no_run
/// use elf_loader::{Loader, ElfBinary};
///
/// let mut loader = Loader::new();
/// let bytes = std::fs::read("liba.so").unwrap();
/// let lib = loader.load_dylib(ElfBinary::new("liba.so", &bytes)).unwrap();
/// ```
pub struct Loader<M, H, D = ()>
where
    M: Mmap,
    H: LoadHook<D>,
    D: Default + 'static,
{
    pub(crate) buf: ElfBuf,
    pub(crate) init_fn: FnHandler,
    pub(crate) fini_fn: FnHandler,
    pub(crate) hook: H,
    _marker: PhantomData<(M, D)>,
}

impl Loader<DefaultMmap, (), ()> {
    /// Creates a new `Loader` with default settings.
    pub fn new() -> Self {
        let c_abi = Arc::new(|func: Option<fn()>, func_array: Option<&[fn()]>| {
            func.iter()
                .chain(func_array.unwrap_or(&[]).iter())
                .for_each(|init| unsafe { core::mem::transmute::<_, &extern "C" fn()>(init) }());
        });
        Self {
            hook: (),
            init_fn: c_abi.clone(),
            fini_fn: c_abi,
            buf: ElfBuf::new(),
            _marker: PhantomData,
        }
    }
}

impl<M: Mmap, H: LoadHook<D>, D: Default + 'static> Loader<M, H, D> {
    /// Sets the initialization function handler.
    ///
    /// This handler is responsible for calling the initialization functions
    /// (e.g., `.init` and `.init_array`) of the loaded ELF object.
    ///
    /// Note: glibc passes `argc`, `argv`, and `envp` to functions in `.init_array`
    /// as a non-standard extension.
    pub fn with_init(&mut self, init_fn: FnHandler) -> &mut Self {
        self.init_fn = init_fn;
        self
    }

    /// Sets the finalization function handler.
    ///
    /// This handler is responsible for calling the finalization functions
    /// (e.g., `.fini` and `.fini_array`) of the loaded ELF object.
    pub fn with_fini(&mut self, fini_fn: FnHandler) -> &mut Self {
        self.fini_fn = fini_fn;
        self
    }

    /// Consumes the current loader and returns a new one with the specified hook.
    ///
    /// This allows replacing the hook type and user data type.
    ///
    /// # Type Parameters
    /// * `NewD` - The new user data type.
    /// * `NewHook` - The new hook type.
    pub fn with_hook<NewD, NewHook>(self, hook: NewHook) -> Loader<M, NewHook, NewD>
    where
        NewD: Default,
        NewHook: LoadHook<NewD>,
    {
        Loader {
            buf: self.buf,
            init_fn: self.init_fn,
            fini_fn: self.fini_fn,
            hook,
            _marker: PhantomData,
        }
    }

    /// Returns a new loader with a custom `Mmap` implementation.
    pub fn with_mmap<NewMmap: Mmap>(self) -> Loader<NewMmap, H, D> {
        Loader {
            buf: self.buf,
            init_fn: self.init_fn,
            fini_fn: self.fini_fn,
            hook: self.hook,
            _marker: PhantomData,
        }
    }

    /// Reads the ELF header.
    pub fn read_ehdr(&mut self, object: &mut impl ElfReader) -> Result<ElfHeader> {
        self.buf.prepare_ehdr(object)
    }

    /// Reads the program header table.
    pub fn read_phdr(
        &mut self,
        object: &mut impl ElfReader,
        ehdr: &ElfHeader,
    ) -> Result<&[ElfPhdr]> {
        self.buf.prepare_phdrs(ehdr, object)
    }

    pub(crate) fn load_static_impl(
        hook: &H,
        init_fn: &FnHandler,
        fini_fn: &FnHandler,
        ehdr: ElfHeader,
        phdrs: &[ElfPhdr],
        mut object: impl ElfReader,
    ) -> Result<StaticImage<D>> {
        let init_fn = init_fn.clone();
        let fini_fn = fini_fn.clone();
        let mut phdr_segments = PhdrSegments::new(phdrs, ehdr.is_dylib(), object.as_fd().is_some());
        let segments = phdr_segments.load_segments::<M>(&mut object)?;
        phdr_segments.mprotect::<M>()?;
        let builder: ImageBuilder<'_, H, M, D> = ImageBuilder::new(
            hook,
            segments,
            object.shortname().to_owned(),
            ehdr,
            init_fn,
            fini_fn,
        );
        Ok(builder.build_static(phdrs)?)
    }

    pub(crate) fn load_dynamic_impl(
        hook: &H,
        init_fn: &FnHandler,
        fini_fn: &FnHandler,
        ehdr: ElfHeader,
        phdrs: &[ElfPhdr],
        mut object: impl ElfReader,
    ) -> Result<DynamicImage<D>> {
        let init_fn = init_fn.clone();
        let fini_fn = fini_fn.clone();
        let mut phdr_segments = PhdrSegments::new(phdrs, ehdr.is_dylib(), object.as_fd().is_some());
        let segments = phdr_segments.load_segments::<M>(&mut object)?;
        phdr_segments.mprotect::<M>()?;
        let builder: ImageBuilder<'_, H, M, D> = ImageBuilder::new(
            hook,
            segments,
            object.shortname().to_owned(),
            ehdr,
            init_fn,
            fini_fn,
        );
        Ok(builder.build_dynamic(phdrs)?)
    }

    /// Load a relocatable ELF object
    pub(crate) fn load_object_impl(
        &mut self,
        ehdr: ElfHeader,
        mut object: impl ElfReader,
    ) -> Result<ObjectImage> {
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
        let builder = ObjectBuilder::new(
            object.shortname().to_owned(),
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
