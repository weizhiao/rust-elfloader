//! Dynamic library (shared object) handling
//!
//! This module provides functionality for working with dynamic libraries
//! (shared objects) that have been loaded but not yet relocated. It includes
//! support for synchronous loading of dynamic libraries.

use super::RelocatedCommonPart;
use crate::{
    Loader, Result, UserData,
    format::Relocated,
    mmap::Mmap,
    object::ElfObject,
    parse_ehdr_error,
    relocation::{Relocatable, RelocationHandler, SymbolLookup, dynamic_link::LazyScope},
};
use core::{fmt::Debug, ops::Deref};

/// An unrelocated dynamic library
///
/// This structure represents a dynamic library (shared object) that has been
/// loaded into memory but has not yet undergone relocation. It contains all
/// the necessary information to perform relocation and prepare the library
/// for execution.
pub struct ElfDylib {
    /// The common part containing basic ELF object information
    inner: RelocatedCommonPart,
}

impl Deref for ElfDylib {
    type Target = RelocatedCommonPart;

    /// Dereferences to the underlying RelocatedCommonPart
    ///
    /// This implementation allows direct access to the common ELF object
    /// fields through the ElfDylib wrapper.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Debug for ElfDylib {
    /// Formats the ElfDylib for debugging purposes
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

#[cfg(not(feature = "portable-atomic"))]
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

impl Relocatable for ElfDylib {
    type Output = RelocatedDylib;

    fn relocate<S, PreH, PostH>(
        self,
        scope: &[Relocated],
        pre_find: &S,
        pre_handler: PreH,
        post_handler: PostH,
        lazy: Option<bool>,
        lazy_scope: Option<LazyScope>,
        use_scope_as_lazy: bool,
    ) -> Result<Self::Output>
    where
        S: SymbolLookup + ?Sized,
        PreH: RelocationHandler,
        PostH: RelocationHandler,
    {
        let (relocated, _) = self.inner.relocate_impl(
            scope,
            pre_find,
            pre_handler,
            post_handler,
            lazy,
            lazy_scope,
            use_scope_as_lazy,
        )?;
        Ok(relocated)
    }
}

impl ElfDylib {
    /// Gets mutable user data from the ELF object
    ///
    /// This method provides access to the user-defined data associated
    /// with this ELF object, allowing modification of the data.
    ///
    /// # Returns
    /// * `Some(user_data)` - A mutable reference to the user data if available
    /// * `None` - If the user data is not available (e.g., already borrowed)
    #[inline]
    pub fn user_data_mut(&mut self) -> Option<&mut UserData> {
        self.inner.user_data_mut()
    }
}

impl<M: Mmap> Loader<M> {
    /// Load a dynamic library into memory
    ///
    /// This method loads a dynamic library (shared object) file into memory
    /// and prepares it for relocation. The file is validated to ensure it
    /// is indeed a dynamic library.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load as a dynamic library
    ///
    /// # Returns
    /// * `Ok(ElfDylib)` - The loaded dynamic library
    /// * `Err(Error)` - If loading fails
    pub fn load_dylib(&mut self, mut object: impl ElfObject) -> Result<ElfDylib> {
        // Prepare and validate the ELF header
        let ehdr = self.buf.prepare_ehdr(&mut object)?;

        // Ensure the file is actually a dynamic library
        if !ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }

        // Load the relocated common part
        let inner = self.load_relocated(ehdr, object)?;

        // Wrap in ElfDylib and return
        Ok(ElfDylib { inner })
    }
}

/// Type alias for a relocated dynamic library
///
/// This type represents a dynamic library that has been loaded and relocated
/// in memory, making it ready for symbol resolution and execution.
pub type RelocatedDylib = Relocated;
