//! ELF file format handling
//!
//! This module provides the core data structures and functionality for working
//! with ELF files in various stages of processing: from raw ELF files to
//! relocated and loaded libraries or executables.

use crate::{
    LoadHook, Loader, Result,
    mmap::Mmap,
    reader::ElfReader,
    relocation::{Relocatable, RelocationHandler, Relocator, SymbolLookup},
};
use core::fmt::Debug;
use elf::abi::{PT_DYNAMIC, PT_INTERP};

mod component;
mod image;
mod object;

pub(crate) use component::ModuleInner;
pub(crate) use image::{DynamicImage, ImageBuilder, StaticImage};
pub(crate) use object::ObjectBuilder;

pub use component::{ElfModule, ElfModuleRef, LoadedModule, Symbol};
pub use image::{DylibImage, ExecImage, LoadedDylib, LoadedExec};
pub use object::{LoadedObject, ObjectImage};

/// A mapped but unrelocated ELF image.
///
/// This enum represents an ELF file that has been loaded into memory (mapped)
/// but has not yet undergone the relocation process. It can be a dynamic library,
/// an executable, or a relocatable object file.
#[derive(Debug)]
pub enum ElfImage<D>
where
    D: 'static,
{
    /// A dynamic library (shared object, typically `.so`).
    Dylib(DylibImage<D>),

    /// An executable file (typically a PIE or non-PIE executable).
    Exec(ExecImage<D>),

    /// A relocatable object file (typically `.o`).
    Object(ObjectImage),
}

/// A fully relocated and ready-to-use ELF module.
///
/// This enum represents an ELF file that has been loaded, mapped, and had all
/// its relocations resolved. It is now ready for symbol lookup or execution.
///
/// It maintains internal reference counts to its dependencies to ensure
/// memory safety during its lifetime.
#[derive(Debug, Clone)]
pub enum LoadedElf<D> {
    /// A relocated dynamic library.
    Dylib(LoadedDylib<D>),

    /// A relocated executable.
    Exec(LoadedExec<D>),

    /// A relocated object file.
    Object(LoadedObject<()>),
}

impl<D: 'static> ElfImage<D> {
    /// Creates a builder for relocating the ELF file.
    ///
    /// # Examples
    /// ```no_run
    /// use elf_loader::{Loader, ElfBinary};
    ///
    /// let mut loader = Loader::new();
    /// let bytes = &[]; // ELF file bytes
    /// let lib = loader.load_dylib(ElfBinary::new("liba.so", bytes)).unwrap();
    ///
    /// let relocated = lib.relocator()
    ///     .lazy(true)
    ///     .relocate()
    ///     .unwrap();
    /// ```
    pub fn relocator(self) -> Relocator<Self, (), (), (), (), (), D> {
        Relocator::new(self)
    }
}

impl<D> LoadedElf<D> {
    /// Converts this LoadedElf into a LoadedDylib if it is one
    ///
    /// # Returns
    /// * `Some(dylib)` - If this is a Dylib variant
    /// * `None` - If this is an Exec variant
    #[inline]
    pub fn into_dylib(self) -> Option<LoadedDylib<D>> {
        match self {
            LoadedElf::Dylib(dylib) => Some(dylib),
            _ => None,
        }
    }

    /// Converts this LoadedElf into a LoadedExec if it is one
    ///
    /// # Returns
    /// * `Some(exec)` - If this is an Exec variant
    /// * `None` - If this is a Dylib variant
    #[inline]
    pub fn into_exec(self) -> Option<LoadedExec<D>> {
        match self {
            LoadedElf::Exec(exec) => Some(exec),
            _ => None,
        }
    }

    /// Gets a reference to the LoadedDylib if this is one
    ///
    /// # Returns
    /// * `Some(dylib)` - If this is a Dylib variant
    /// * `None` - If this is an Exec variant
    #[inline]
    pub fn as_dylib(&self) -> Option<&LoadedDylib<D>> {
        match self {
            LoadedElf::Dylib(dylib) => Some(dylib),
            _ => None,
        }
    }
}

impl<D: 'static> Relocatable<D> for ElfImage<D> {
    type Output = LoadedElf<D>;

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
        D: 'static,
        PreS: SymbolLookup + ?Sized,
        PostS: SymbolLookup + ?Sized,
        LazyS: SymbolLookup + Send + Sync + 'static,
        PreH: RelocationHandler,
        PostH: RelocationHandler,
    {
        match self {
            ElfImage::Dylib(dylib) => {
                let relocated = Relocatable::relocate(
                    dylib,
                    scope,
                    pre_find,
                    post_find,
                    pre_handler,
                    post_handler,
                    lazy,
                    lazy_scope,
                )?;
                Ok(LoadedElf::Dylib(relocated))
            }
            ElfImage::Exec(exec) => {
                let relocated = Relocatable::relocate(
                    exec,
                    scope,
                    pre_find,
                    post_find,
                    pre_handler,
                    post_handler,
                    lazy,
                    lazy_scope,
                )?;
                Ok(LoadedElf::Exec(relocated))
            }
            ElfImage::Object(relocatable) => {
                let relocated = Relocatable::relocate(
                    relocatable,
                    &[],
                    pre_find,
                    post_find,
                    pre_handler,
                    post_handler,
                    lazy,
                    None::<LazyS>, // ElfRelocatable always uses LazyScope<(), ()>, so pass None
                )?;
                Ok(LoadedElf::Object(relocated))
            }
        }
    }
}

impl<M: Mmap, H: LoadHook<D>, D: Default + 'static> Loader<M, H, D> {
    /// Load an ELF file into memory
    ///
    /// # Arguments
    /// * `object` - The ELF object to load
    ///
    /// # Returns
    /// * `Ok(Elf)` - The loaded ELF file
    /// * `Err(Error)` - If loading fails
    pub fn load(&mut self, mut object: impl ElfReader) -> Result<ElfImage<D>> {
        let ehdr = self.buf.prepare_ehdr(&mut object)?;

        if ehdr.e_type == elf::abi::ET_REL {
            // Relocatable files don't use user_data, so we call load_rel directly
            return Ok(ElfImage::Object(self.load_object_impl(ehdr, object)?));
        }

        let phdrs = self.buf.prepare_phdrs(&ehdr, &mut object)?;
        let has_dynamic = phdrs.iter().any(|p| p.p_type == PT_DYNAMIC);

        // For ET_DYN, we check if it has an interpreter (PT_INTERP)
        // or if it lacks a dynamic section (static PIE)
        // to distinguish between PIE executables and shared libraries.
        let is_pie =
            ehdr.is_dylib() && (phdrs.iter().any(|p| p.p_type == PT_INTERP) || !has_dynamic);

        let is_exec = ehdr.e_type == elf::abi::ET_EXEC || is_pie;

        if is_exec {
            let exec = if has_dynamic {
                ExecImage::Dynamic(Self::load_dynamic_impl(
                    &self.hook,
                    &self.init_fn,
                    &self.fini_fn,
                    ehdr,
                    phdrs,
                    object,
                )?)
            } else {
                ExecImage::Static(Self::load_static_impl(
                    &self.hook,
                    &self.init_fn,
                    &self.fini_fn,
                    ehdr,
                    phdrs,
                    object,
                )?)
            };
            Ok(ElfImage::Exec(exec))
        } else if ehdr.is_dylib() {
            let inner = Self::load_dynamic_impl(
                &self.hook,
                &self.init_fn,
                &self.fini_fn,
                ehdr,
                phdrs,
                object,
            )?;
            Ok(ElfImage::Dylib(DylibImage { inner }))
        } else {
            // Fallback for other types, though usually handled by is_exec
            Ok(ElfImage::Exec(self.load_exec(object)?))
        }
    }
}
