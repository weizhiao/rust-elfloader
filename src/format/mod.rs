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
use core::{fmt::Debug, marker::PhantomData, ops::Deref};

mod component;
mod dynamic;
mod object;

pub(crate) use component::ModuleInner;
pub(crate) use dynamic::{DynamicBuilder, DynamicComponent};
pub(crate) use object::ObjectBuilder;

pub use component::{ElfModule, ElfModuleRef, LoadedModule};
pub use dynamic::{DylibImage, ExecImage, LoadedDylib, LoadedExec};
pub use object::{LoadedObject, ObjectImage};

/// An unrelocated ELF file.
///
/// This enum represents an ELF file that has been loaded into memory but
/// has not yet undergone relocation. It can be either a dynamic library,
/// an executable, or a relocatable object file.
#[derive(Debug)]
pub enum ElfImage<D>
where
    D: 'static,
{
    /// A dynamic library (shared object, `.so`).
    Dylib(DylibImage<D>),

    /// An executable file.
    Exec(ExecImage<D>),

    /// A relocatable object file (`.o`).
    Object(ObjectImage),
}

/// An ELF file that has been relocated and is ready for execution.
///
/// This enum represents an ELF file that has been loaded and relocated.
/// It maintains dependency information to ensure that required libraries
/// are not deallocated while this ELF is still in use.
#[derive(Debug, Clone)]
pub enum LoadedElf<D> {
    /// A relocated dynamic library.
    Dylib(LoadedDylib<D>),

    /// A relocated executable.
    Exec(LoadedExec<D>),

    /// A relocated relocatable file.
    Object(LoadedObject<()>),
}

impl<D: 'static> ElfImage<D> {
    /// Creates a builder for relocating the ELF file.
    ///
    /// This method returns a [`Relocator`] that allows configuring the relocation
    /// process with fine-grained control, such as:
    /// * Providing custom symbol resolution strategies.
    /// * Handling unknown relocations.
    /// * Configuring lazy or eager binding.
    /// * Specifying the symbol resolution scope.
    pub fn relocator(self) -> Relocator<Self, (), (), (), (), (), D> {
        Relocator::new(self)
    }
}

impl<D> LoadedElf<D> {
    /// Converts this RelocatedElf into a RelocatedDylib if it is one
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

    /// Converts this RelocatedElf into a RelocatedExec if it is one
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

    /// Gets a reference to the RelocatedDylib if this is one
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

impl<D> Deref for ElfImage<D> {
    type Target = ElfModule<D>;

    /// Dereferences to the underlying CoreComponent
    ///
    /// This allows direct access to common fields shared by all ELF file types.
    ///
    /// # Panics
    /// Panics if called on a Relocatable variant, as relocatable files always use `CoreComponent<()>`.
    fn deref(&self) -> &Self::Target {
        match self {
            ElfImage::Dylib(elf_dylib) => elf_dylib.core_ref(),
            ElfImage::Exec(elf_exec) => elf_exec.core_ref(),
            ElfImage::Object(_) => panic!("Deref not supported for Relocatable variant"),
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
        use_scope_as_lazy: bool,
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
                    use_scope_as_lazy,
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
                    use_scope_as_lazy,
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
                    use_scope_as_lazy,
                )?;
                Ok(LoadedElf::Object(relocated))
            }
        }
    }
}

/// A symbol from an ELF object
///
/// A symbol loaded from an ELF file.
///
/// This structure represents a symbol loaded from an ELF file, such as a
/// function or global variable. It provides safe access to the symbol
/// while maintaining proper lifetime information.
///
/// The type parameter `T` represents the type of the symbol (e.g., a function
/// signature or a variable type).
#[derive(Debug, Clone)]
pub struct Symbol<'lib, T: 'lib> {
    /// Raw pointer to the symbol data.
    pub(crate) ptr: *mut (),

    /// Phantom data to maintain lifetime information.
    pd: PhantomData<&'lib T>,
}

impl<'lib, T> Deref for Symbol<'lib, T> {
    type Target = T;

    /// Dereferences to the underlying symbol type.
    ///
    /// This allows direct use of the symbol as if it were of type `T`.
    ///
    /// # Returns
    /// A reference to the symbol of type `T`.
    fn deref(&self) -> &T {
        unsafe { &*(&self.ptr as *const *mut _ as *const T) }
    }
}

impl<'lib, T> Symbol<'lib, T> {
    /// Consumes the symbol and returns the raw pointer
    ///
    /// This method converts the symbol into a raw pointer, transferring
    /// ownership to the caller.
    ///
    /// # Returns
    /// A raw pointer to the symbol data
    pub fn into_raw(self) -> *const () {
        self.ptr
    }
}

// Safety: Symbol can be sent between threads if T can
unsafe impl<T: Send> Send for Symbol<'_, T> {}

// Safety: Symbol can be shared between threads if T can
unsafe impl<T: Sync> Sync for Symbol<'_, T> {}

impl<M: Mmap, H: LoadHook<D>, D: Default> Loader<M, H, D> {
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
        let is_dylib = ehdr.is_dylib();
        if is_dylib {
            Ok(ElfImage::Dylib(self.load_dylib(object)?))
        } else if ehdr.e_type == elf::abi::ET_REL {
            // Relocatable files don't use user_data, so we call load_rel directly
            Ok(ElfImage::Object(self.load_rel(ehdr, object)?))
        } else {
            Ok(ElfImage::Exec(self.load_exec(object)?))
        }
    }
}
