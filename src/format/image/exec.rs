/// Executable file handling
///
/// This module provides functionality for working with executable ELF files
/// that have been loaded but not yet relocated. It includes support for
/// synchronous loading of executable files.
use crate::{
    LoadHook, Loader, Result,
    format::{LoadedModule, image::common::DynamicImage},
    mmap::Mmap,
    parse_ehdr_error,
    reader::ElfReader,
    relocation::{Relocatable, RelocationHandler, Relocator, SymbolLookup},
    segment::ElfSegments,
};
use alloc::string::String;
use core::fmt::Debug;
use elf::abi::PT_DYNAMIC;

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

#[derive(Clone)]
pub struct StaticImage<D> {
    pub(crate) inner: Arc<StaticImageInner<D>>,
}

impl<D> Debug for StaticImage<D> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StaticImage")
            .field("name", &self.inner.name)
            .finish()
    }
}

impl<D> StaticImage<D> {
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    pub fn entry(&self) -> usize {
        self.inner.entry
    }
}

pub(crate) struct StaticImageInner<D> {
    /// File name of the ELF object
    pub(crate) name: String,

    pub(crate) entry: usize,

    /// User-defined data
    pub(crate) user_data: D,

    /// Memory segments
    pub(crate) segments: ElfSegments,
}

impl<D: 'static> Relocatable<D> for ExecImage<D> {
    type Output = LoadedExec<D>;

    fn relocate<PreS, PostS, LazyS, PreH, PostH>(
        self,
        scope: &[LoadedModule<D>],
        pre_find: &PreS,
        post_find: &PostS,
        pre_handler: PreH,
        post_handler: PostH,
        lazy: Option<bool>,
        lazy_scope: Option<LazyS>,
    ) -> Result<Self::Output>
    where
        PreS: SymbolLookup + ?Sized,
        PostS: SymbolLookup + ?Sized,
        LazyS: SymbolLookup + Send + Sync + 'static,
        PreH: RelocationHandler,
        PostH: RelocationHandler,
    {
        match self {
            ExecImage::Dynamic(image) => {
                let entry = image.entry();
                let inner = image.relocate_impl(
                    scope,
                    pre_find,
                    post_find,
                    pre_handler,
                    post_handler,
                    lazy,
                    lazy_scope,
                )?;
                Ok(LoadedExec {
                    entry,
                    inner: LoadedExecInner::Dynamic(inner),
                })
            }
            ExecImage::Static(image) => Ok(LoadedExec {
                entry: image.entry(),
                inner: LoadedExecInner::Static(image),
            }),
        }
    }
}

/// An unrelocated executable file.
///
/// This structure represents an executable ELF file that has been loaded
/// into memory but has not yet undergone relocation. It contains all the
/// necessary information to perform relocation and prepare the executable
/// for execution.
pub enum ExecImage<D>
where
    D: 'static,
{
    /// The common part containing basic ELF object information.
    Dynamic(DynamicImage<D>),
    Static(StaticImage<D>),
}

impl<D> Debug for ExecImage<D> {
    /// Formats the [`ExecImage`] for debugging purposes.
    ///
    /// This implementation provides a debug representation that includes
    /// the executable name and its dependencies.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ElfExec")
            .field("name", &self.name())
            .finish()
    }
}

impl<D: 'static> ExecImage<D> {
    /// Creates a builder for relocating the executable.
    pub fn relocator(self) -> Relocator<Self, (), (), (), (), (), D> {
        Relocator::new(self)
    }

    pub fn name(&self) -> &str {
        match self {
            ExecImage::Dynamic(image) => image.name(),
            ExecImage::Static(image) => image.name(),
        }
    }
}

impl<M: Mmap, H: LoadHook<D>, D: Default> Loader<M, H, D> {
    /// Loads an executable file into memory.
    ///
    /// This method loads an executable ELF file into memory and prepares it
    /// for relocation. The file is validated to ensure it is indeed an
    /// executable (either a standard executable or a position-independent executable).
    ///
    /// # Arguments
    /// * `object` - The ELF object to load as an executable.
    ///
    /// # Returns
    /// * `Ok(ExecImage)` - The loaded executable.
    /// * `Err(Error)` - If loading fails.
    ///
    /// # Examples
    /// ```no_run
    /// use elf_loader::{Loader, ElfBinary};
    ///
    /// let mut loader = Loader::new();
    /// let bytes = &[]; // ELF executable bytes
    /// let exec = loader.load_exec(ElfBinary::new("my_exec", bytes)).unwrap();
    /// ```
    pub fn load_exec(&mut self, mut object: impl ElfReader) -> Result<ExecImage<D>> {
        // Prepare and validate the ELF header
        let ehdr = self.buf.prepare_ehdr(&mut object)?;

        // Ensure the file is actually an executable
        if !ehdr.is_executable() {
            return Err(parse_ehdr_error("file type mismatch"));
        }

        let phdrs = self.buf.prepare_phdrs(&ehdr, &mut object)?;
        let has_dynamic = phdrs.iter().any(|phdr| phdr.p_type == PT_DYNAMIC);

        if has_dynamic {
            // Load the relocated common part
            let inner = Self::load_dynamic_impl(
                &self.hook,
                &self.init_fn,
                &self.fini_fn,
                ehdr,
                phdrs,
                object,
            )?;
            // Wrap in ElfExec and return
            Ok(ExecImage::Dynamic(inner))
        } else {
            // Load as a static module without dynamic section
            let inner = Self::load_static_impl(
                &self.hook,
                &self.init_fn,
                &self.fini_fn,
                ehdr,
                phdrs,
                object,
            )?;
            Ok(ExecImage::Static(inner))
        }
    }
}

/// An executable file that has been relocated.
///
/// This structure represents an executable ELF file that has been loaded
/// and relocated in memory, making it ready for execution. It contains
/// the entry point and other information needed to run the executable.
#[derive(Clone, Debug)]
pub struct LoadedExec<D> {
    /// Entry point of the executable.
    entry: usize,
    /// The relocated ELF object.
    inner: LoadedExecInner<D>,
}

#[derive(Clone, Debug)]
enum LoadedExecInner<D> {
    Dynamic(LoadedModule<D>),
    Static(StaticImage<D>),
}

impl<D> LoadedExec<D> {
    /// Returns the entry point of the executable.
    ///
    /// # Returns
    /// The virtual address of the entry point.
    #[inline]
    pub fn entry(&self) -> usize {
        self.entry
    }

    pub fn mapped_len(&self) -> usize {
        match &self.inner {
            LoadedExecInner::Dynamic(module) => module.mapped_len(),
            LoadedExecInner::Static(static_image) => static_image.inner.segments.len(),
        }
    }

    pub fn user_data(&self) -> &D {
        match &self.inner {
            LoadedExecInner::Dynamic(module) => &module.user_data(),
            LoadedExecInner::Static(static_image) => &static_image.inner.user_data,
        }
    }

    pub fn is_static(&self) -> bool {
        match &self.inner {
            LoadedExecInner::Dynamic(_) => false,
            LoadedExecInner::Static(_) => true,
        }
    }

    pub fn module_ref(&self) -> Option<&LoadedModule<D>> {
        match &self.inner {
            LoadedExecInner::Dynamic(module) => Some(module),
            LoadedExecInner::Static(_) => None,
        }
    }
}
