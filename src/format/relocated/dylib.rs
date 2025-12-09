//! Dynamic library (shared object) handling
//!
//! This module provides functionality for working with dynamic libraries
//! (shared objects) that have been loaded but not yet relocated. It includes
//! support for both synchronous and asynchronous loading of dynamic libraries.

use super::RelocatedCommonPart;
use crate::{
    Loader, Result, UserData,
    format::{Relocated, create_lazy_scope},
    mmap::Mmap,
    object::{ElfObject, ElfObjectAsync},
    parse_ehdr_error,
    relocation::dynamic_link::{LazyScope, UnknownHandler, relocate_impl},
};
use alloc::{boxed::Box, vec::Vec};
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

    /// Relocate the dynamic library with the given dynamic libraries and function closure
    ///
    /// This is a convenience method that performs relocation with default settings.
    /// It creates a local lazy scope if lazy binding is enabled for this library.
    ///
    /// # Note
    /// During relocation, the symbol is first searched in the function closure `pre_find`.
    ///
    /// # Arguments
    /// * `scope` - Iterator over relocated libraries to use for symbol resolution
    /// * `pre_find` - Function to use for initial symbol lookup
    ///
    /// # Returns
    /// * `Ok(RelocatedDylib)` - The relocated dynamic library
    /// * `Err(Error)` - If relocation fails
    pub fn easy_relocate<'iter, 'scope, 'find, 'lib, F>(
        self,
        scope: impl IntoIterator<Item = &'iter Relocated<'scope>>,
        pre_find: &'find F,
    ) -> Result<RelocatedDylib<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        let iter = scope.into_iter();
        let mut helper = Vec::new();
        let local_lazy_scope = if self.is_lazy() {
            let mut libs = Vec::new();
            iter.for_each(|lib| {
                libs.push(lib.downgrade());
                helper.push(lib);
            });
            Some(create_lazy_scope(libs, pre_find))
        } else {
            iter.for_each(|lib| {
                helper.push(lib);
            });
            None
        };
        self.relocate(
            helper,
            pre_find,
            &mut |_, _, _| Err(Box::new(())),
            local_lazy_scope,
        )
    }

    /// Relocate the dynamic library with the given dynamic libraries and function closure
    ///
    /// This method provides full control over the relocation process, allowing
    /// custom handling of unknown relocations and specification of a local
    /// lazy scope.
    ///
    /// # Note
    /// * During relocation, the symbol is first searched in the function closure `pre_find`.
    /// * The `deal_unknown` function is used to handle relocation types not implemented by elf_loader or failed relocations
    /// * Typically, the `scope` should also contain the current dynamic library itself,
    ///   relocation will be done in the exact order in which the dynamic libraries appear in `scope`.
    /// * When lazy binding, the symbol is first looked for in the global scope and then in the local lazy scope
    ///
    /// # Arguments
    /// * `scope` - Slice of relocated libraries to use for symbol resolution
    /// * `pre_find` - Function to use for initial symbol lookup
    /// * `deal_unknown` - Handler for unknown or failed relocations
    /// * `local_lazy_scope` - Optional local scope for lazy binding
    ///
    /// # Returns
    /// * `Ok(RelocatedDylib)` - The relocated dynamic library
    /// * `Err(Error)` - If relocation fails
    pub fn relocate<'iter, 'scope, 'find, 'lib, F>(
        self,
        scope: impl AsRef<[&'iter Relocated<'scope>]>,
        pre_find: &'find F,
        deal_unknown: &mut UnknownHandler,
        local_lazy_scope: Option<LazyScope<'lib>>,
    ) -> Result<RelocatedDylib<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        relocate_impl(
            self.inner,
            scope.as_ref(),
            pre_find,
            deal_unknown,
            local_lazy_scope,
        )
    }
}

impl<M: Mmap> Loader<M> {
    /// Load a dynamic library into memory
    ///
    /// This is a convenience method that calls [load_dylib] with `lazy_bind` set to `None`.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load as a dynamic library
    ///
    /// # Returns
    /// * `Ok(ElfDylib)` - The loaded dynamic library
    /// * `Err(Error)` - If loading fails
    pub fn easy_load_dylib(&mut self, object: impl ElfObject) -> Result<ElfDylib> {
        self.load_dylib(object, None)
    }

    /// Load a dynamic library into memory
    ///
    /// This method loads a dynamic library (shared object) file into memory
    /// and prepares it for relocation. The file is validated to ensure it
    /// is indeed a dynamic library.
    ///
    /// # Note
    /// When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load as a dynamic library
    /// * `lazy_bind` - Optional override for lazy binding behavior
    ///
    /// # Returns
    /// * `Ok(ElfDylib)` - The loaded dynamic library
    /// * `Err(Error)` - If loading fails
    pub fn load_dylib(
        &mut self,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
    ) -> Result<ElfDylib> {
        // Prepare and validate the ELF header
        let ehdr = self.buf.prepare_ehdr(&mut object)?;

        // Ensure the file is actually a dynamic library
        if !ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }

        // Load the relocated common part
        let inner = self.load_relocated(ehdr, object, lazy_bind)?;

        // Wrap in ElfDylib and return
        Ok(ElfDylib { inner })
    }

    /// Load a dynamic library into memory asynchronously
    ///
    /// This method loads a dynamic library (shared object) file into memory
    /// asynchronously and prepares it for relocation. The file is validated
    /// to ensure it is indeed a dynamic library.
    ///
    /// # Note
    /// When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load as a dynamic library
    /// * `lazy_bind` - Optional override for lazy binding behavior
    ///
    /// # Returns
    /// * `Ok(ElfDylib)` - The loaded dynamic library
    /// * `Err(Error)` - If loading fails
    pub async fn load_dylib_async(
        &mut self,
        mut object: impl ElfObjectAsync,
        lazy_bind: Option<bool>,
    ) -> Result<ElfDylib> {
        // Prepare and validate the ELF header
        let ehdr = self.buf.prepare_ehdr(&mut object)?;

        // Ensure the file is actually a dynamic library
        if !ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }

        // Load the relocated common part asynchronously
        let inner = self.load_relocated_async(ehdr, object, lazy_bind).await?;

        // Wrap in ElfDylib and return
        Ok(ElfDylib { inner })
    }
}

/// Type alias for a relocated dynamic library
///
/// This type represents a dynamic library that has been loaded and relocated
/// in memory, making it ready for symbol resolution and execution.
pub type RelocatedDylib<'lib> = Relocated<'lib>;
