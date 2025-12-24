//! Executable file handling
//!
//! This module provides functionality for working with executable ELF files
//! that have been loaded but not yet relocated. It includes support for
//! synchronous loading of executable files.

use super::RelocatedCommonPart;
use crate::{
    CoreComponent, Hook, Loader, Result,
    format::Relocated,
    mmap::Mmap,
    object::ElfObject,
    parse_ehdr_error,
    relocation::{Relocatable, RelocationHandler, Relocator, SymbolLookup},
};
use core::{fmt::Debug, ops::Deref};

impl<D: 'static> Relocatable<D> for ElfExec<D> {
    type Output = RelocatedExec<D>;

    fn relocate<PreS, PostS, LazyS, PreH, PostH>(
        self,
        scope: &[Relocated<D>],
        pre_find: &PreS,
        post_find: &PostS,
        pre_handler: PreH,
        post_handler: PostH,
        lazy: Option<bool>,
        lazy_scope: Option<LazyS>,
        use_scope_as_lazy: bool,
    ) -> Result<Self::Output>
    where
        PreS: SymbolLookup + ?Sized,
        PostS: SymbolLookup + ?Sized,
        LazyS: SymbolLookup + Send + Sync + 'static,
        PreH: RelocationHandler,
        PostH: RelocationHandler,
    {
        let (relocated, entry) = self.inner.relocate_impl(
            scope,
            pre_find,
            post_find,
            pre_handler,
            post_handler,
            lazy,
            lazy_scope,
            use_scope_as_lazy,
        )?;
        Ok(RelocatedExec {
            entry,
            inner: relocated,
        })
    }
}

impl<D> Deref for ElfExec<D> {
    type Target = RelocatedCommonPart<D>;

    /// Dereferences to the underlying RelocatedCommonPart
    ///
    /// This implementation allows direct access to the common ELF object
    /// fields through the ElfExec wrapper.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// An unrelocated executable file.
///
/// This structure represents an executable ELF file that has been loaded
/// into memory but has not yet undergone relocation. It contains all the
/// necessary information to perform relocation and prepare the executable
/// for execution.
pub struct ElfExec<D>
where
    D: 'static,
{
    /// The common part containing basic ELF object information.
    inner: RelocatedCommonPart<D>,
}

impl<D> Debug for ElfExec<D> {
    /// Formats the [`ElfExec`] for debugging purposes.
    ///
    /// This implementation provides a debug representation that includes
    /// the executable name and its dependencies.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ElfExec")
            .field("name", &self.inner.name())
            .field("needed_libs", &self.inner.needed_libs())
            .finish()
    }
}

impl<D: 'static> ElfExec<D> {
    /// Creates a builder for relocating the executable.
    ///
    /// This method returns a [`Relocator`] that allows configuring the relocation
    /// process with fine-grained control, such as setting a custom unknown relocation
    /// handler, forcing lazy/eager binding, and specifying the symbol resolution scope.
    pub fn relocator(self) -> Relocator<Self, (), (), (), (), (), D> {
        Relocator::new(self)
    }
}

impl<M: Mmap, H: Hook<D>, D: Default> Loader<M, H, D> {
    /// Loads an executable file into memory.
    ///
    /// This method loads an executable ELF file into memory and prepares it
    /// for relocation. The file is validated to ensure it is indeed an
    /// executable and not a dynamic library.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load as an executable.
    ///
    /// # Returns
    /// * `Ok(ElfExec)` - The loaded executable.
    /// * `Err(Error)` - If loading fails.
    ///
    /// # Examples
    /// ```no_run
    /// use elf_loader::{Loader, object::ElfBinary};
    ///
    /// let mut loader = Loader::new();
    /// let bytes = &[]; // ELF executable bytes
    /// let exec = loader.load_exec(ElfBinary::new("my_exec", bytes)).unwrap();
    /// ```
    pub fn load_exec(&mut self, mut object: impl ElfObject) -> Result<ElfExec<D>> {
        // Prepare and validate the ELF header
        let ehdr = self.buf.prepare_ehdr(&mut object)?;

        // Ensure the file is actually an executable and not a dynamic library
        if ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }

        // Load the relocated common part
        let inner = self.load_relocated(ehdr, object)?;

        // Wrap in ElfExec and return
        Ok(ElfExec { inner })
    }
}

/// An executable file that has been relocated.
///
/// This structure represents an executable ELF file that has been loaded
/// and relocated in memory, making it ready for execution. It contains
/// the entry point and other information needed to run the executable.
#[derive(Clone)]
pub struct RelocatedExec<D> {
    /// Entry point of the executable.
    entry: usize,
    /// The relocated ELF object.
    inner: Relocated<D>,
}

impl<D> RelocatedExec<D> {
    /// Returns the entry point of the executable.
    ///
    /// # Returns
    /// The virtual address of the entry point.
    #[inline]
    pub fn entry(&self) -> usize {
        self.entry
    }
}

impl<D> Debug for RelocatedExec<D> {
    /// Formats the [`RelocatedExec`] for debugging purposes.
    ///
    /// This implementation delegates to the inner [`Relocated`] object's
    /// debug implementation.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<D> Deref for RelocatedExec<D> {
    type Target = CoreComponent<D>;

    /// Dereferences to the underlying [`CoreComponent`].
    ///
    /// This implementation allows direct access to the [`CoreComponent`]
    /// fields through the [`RelocatedExec`] wrapper.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
