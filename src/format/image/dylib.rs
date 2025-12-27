//! Dynamic library (shared object) handling
//!
//! This module provides functionality for working with dynamic libraries
//! (shared objects) that have been loaded but not yet relocated. It includes
//! support for synchronous loading of dynamic libraries.

use crate::{
    LoadHook, Loader, Result,
    format::{LoadedModule, image::common::DynamicImage},
    mmap::Mmap,
    parse_ehdr_error,
    reader::ElfReader,
    relocation::{Relocatable, RelocationHandler, Relocator, SymbolLookup},
};
use core::{fmt::Debug, ops::Deref};

/// An unrelocated dynamic library.
///
/// This structure represents a dynamic library (shared object, `.so`) that has been
/// loaded into memory but has not yet undergone relocation. It contains all
/// the necessary information to perform relocation and prepare the library
/// for execution.
pub struct DylibImage<D>
where
    D: 'static,
{
    /// The common part containing basic ELF object information.
    pub(crate) inner: DynamicImage<D>,
}

impl<D> Deref for DylibImage<D> {
    type Target = DynamicImage<D>;

    /// This implementation allows direct access to the common ELF object
    /// fields through the [`DylibImage`] wrapper.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<D> Debug for DylibImage<D> {
    /// Formats the [`DylibImage`] for debugging purposes.
    ///
    /// This implementation provides a debug representation that includes
    /// the library name and its dependencies.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ElfDylib")
            .field("name", &self.inner.name())
            .field("needed_libs", &self.inner.needed_libs())
            .finish()
    }
}

impl<D> Relocatable<D> for DylibImage<D> {
    type Output = LoadedModule<D>;

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
        let relocated = self.inner.relocate_impl(
            scope,
            pre_find,
            post_find,
            pre_handler,
            post_handler,
            lazy,
            lazy_scope,
        )?;
        Ok(relocated)
    }
}

impl<D> DylibImage<D> {
    /// Returns a mutable reference to the user-defined data associated with this ELF object.
    ///
    /// This method provides access to the user-defined data associated
    /// with this ELF object, allowing modification of the data.
    ///
    /// # Returns
    /// * `Some(user_data)` - A mutable reference to the user data if available.
    /// * `None` - If the user data is not available (e.g., already borrowed).
    #[inline]
    pub fn user_data_mut(&mut self) -> Option<&mut D> {
        self.inner.user_data_mut()
    }

    /// Creates a builder for relocating the dynamic library.
    pub fn relocator(self) -> Relocator<Self, (), (), (), (), (), D> {
        Relocator::new(self)
    }
}

impl<M: Mmap, H: LoadHook<D>, D: Default> Loader<M, H, D> {
    /// Loads a dynamic library into memory.
    ///
    /// This method loads a dynamic library (shared object) file into memory
    /// and prepares it for relocation. The file is validated to ensure it
    /// is indeed a dynamic library.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load as a dynamic library.
    ///
    /// # Returns
    /// * `Ok(DylibImage)` - The loaded dynamic library.
    /// * `Err(Error)` - If loading fails.
    ///
    /// # Examples
    /// ```no_run
    /// use elf_loader::{Loader, ElfBinary};
    ///
    /// let mut loader = Loader::new();
    /// let bytes = &[]; // ELF file bytes
    /// let lib = loader.load_dylib(ElfBinary::new("liba.so", bytes)).unwrap();
    /// ```
    pub fn load_dylib(&mut self, mut object: impl ElfReader) -> Result<DylibImage<D>> {
        // Prepare and validate the ELF header
        let ehdr = self.buf.prepare_ehdr(&mut object)?;

        // Ensure the file is actually a dynamic library
        if !ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }

        let phdrs = self.buf.prepare_phdrs(&ehdr, &mut object)?;

        // Load the relocated common part
        let inner = Self::load_dynamic_impl(
            &self.hook,
            &self.init_fn,
            &self.fini_fn,
            ehdr,
            phdrs,
            object,
        )?;

        // Wrap in ElfDylib and return
        Ok(DylibImage { inner })
    }
}

/// Type alias for a relocated dynamic library.
///
/// This type represents a dynamic library that has been loaded and relocated
/// in memory, making it ready for symbol resolution and execution.
pub type LoadedDylib<D> = LoadedModule<D>;
