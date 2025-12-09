//! Executable file handling
//!
//! This module provides functionality for working with executable ELF files
//! that have been loaded but not yet relocated. It includes support for
//! synchronous loading of executable files.

use super::RelocatedCommonPart;
use crate::{
    CoreComponent, Loader, RelocatedDylib, Result,
    format::{Relocated, create_lazy_scope},
    mmap::Mmap,
    object::ElfObject,
    parse_ehdr_error,
    relocation::dynamic_link::{LazyScope, UnknownHandler, relocate_impl},
};
use alloc::{boxed::Box, vec::Vec};
use core::{fmt::Debug, marker::PhantomData, ops::Deref};

impl Deref for ElfExec {
    type Target = RelocatedCommonPart;

    /// Dereferences to the underlying RelocatedCommonPart
    ///
    /// This implementation allows direct access to the common ELF object
    /// fields through the ElfExec wrapper.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// An unrelocated executable file
///
/// This structure represents an executable ELF file that has been loaded
/// into memory but has not yet undergone relocation. It contains all the
/// necessary information to perform relocation and prepare the executable
/// for execution.
pub struct ElfExec {
    /// The common part containing basic ELF object information
    inner: RelocatedCommonPart,
}

impl Debug for ElfExec {
    /// Formats the ElfExec for debugging purposes
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

impl ElfExec {
    /// Relocate the executable file with the given dynamic libraries and function closure
    ///
    /// This is a convenience method that performs relocation with default settings.
    /// It creates a local lazy scope if lazy binding is enabled for this executable.
    ///
    /// # Note
    /// During relocation, the symbol is first searched in the function closure `pre_find`.
    ///
    /// # Arguments
    /// * `scope` - Iterator over relocated libraries to use for symbol resolution
    /// * `pre_find` - Function to use for initial symbol lookup
    ///
    /// # Returns
    /// * `Ok(RelocatedExec)` - The relocated executable
    /// * `Err(Error)` - If relocation fails
    pub fn easy_relocate<'iter, 'scope, 'find, 'lib, F>(
        self,
        scope: impl IntoIterator<Item = &'iter Relocated<'scope>>,
        pre_find: &'find F,
    ) -> Result<RelocatedExec<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        // If there are no relocations, we can return early
        if self.inner.relocation().is_empty() {
            return Ok(RelocatedExec {
                entry: self.inner.entry,
                inner: Relocated {
                    core: self.inner.into_core_component(),
                    _marker: PhantomData,
                },
            });
        }

        // Create a helper vector to store the relocation scope
        let mut helper: Vec<&Relocated<'_>> = Vec::new();

        // Add the executable itself to the scope if it has a symbol table
        let temp = unsafe { &RelocatedDylib::from_core_component(self.core_component()) };
        if self.inner.symtab().is_some() {
            helper.push(unsafe { core::mem::transmute::<&RelocatedDylib, &RelocatedDylib>(temp) });
        }

        // Process the provided scope
        let iter = scope.into_iter();
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

        // Perform the relocation and return the result
        Ok(RelocatedExec {
            entry: self.inner.entry,
            inner: relocate_impl(
                self.inner,
                &helper,
                pre_find,
                &mut |_, _, _| Err(Box::new(())),
                local_lazy_scope,
            )?,
        })
    }

    /// Relocate the executable file with the given dynamic libraries and function closure
    ///
    /// This method provides full control over the relocation process, allowing
    /// custom handling of unknown relocations and specification of a local
    /// lazy scope.
    ///
    /// # Note
    /// * During relocation, the symbol is first searched in the function closure `pre_find`.
    /// * The `deal_unknown` function is used to handle relocation types not implemented by elf_loader or failed relocations
    /// * Relocation will be done in the exact order in which the dynamic libraries appear in `scope`.
    /// * When lazy binding, the symbol is first looked for in the global scope and then in the local lazy scope
    ///
    /// # Arguments
    /// * `scope` - Slice of relocated libraries to use for symbol resolution
    /// * `pre_find` - Function to use for initial symbol lookup
    /// * `deal_unknown` - Handler for unknown or failed relocations
    /// * `local_lazy_scope` - Optional local scope for lazy binding
    ///
    /// # Returns
    /// * `Ok(RelocatedExec)` - The relocated executable
    /// * `Err(Error)` - If relocation fails
    pub fn relocate<'iter, 'scope, 'find, 'lib, F>(
        self,
        scope: impl AsRef<[&'iter Relocated<'scope>]>,
        pre_find: &'find F,
        deal_unknown: &mut UnknownHandler,
        local_lazy_scope: Option<LazyScope<'lib>>,
    ) -> Result<RelocatedExec<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        // If there are no relocations, we can return early
        if self.inner.relocation().is_empty() {
            return Ok(RelocatedExec {
                entry: self.inner.entry,
                inner: Relocated {
                    core: self.inner.into_core_component(),
                    _marker: PhantomData,
                },
            });
        }

        // Perform the relocation and return the result
        Ok(RelocatedExec {
            entry: self.inner.entry,
            inner: relocate_impl(
                self.inner,
                scope.as_ref(),
                pre_find,
                deal_unknown,
                local_lazy_scope,
            )?,
        })
    }
}

impl<M: Mmap> Loader<M> {
    /// Load an executable file into memory
    ///
    /// This is a convenience method that calls [load_exec] with `lazy_bind` set to `None`.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load as an executable
    ///
    /// # Returns
    /// * `Ok(ElfExec)` - The loaded executable
    /// * `Err(Error)` - If loading fails
    pub fn easy_load_exec(&mut self, object: impl ElfObject) -> Result<ElfExec> {
        self.load_exec(object, None)
    }

    /// Load an executable file into memory
    ///
    /// This method loads an executable ELF file into memory and prepares it
    /// for relocation. The file is validated to ensure it is indeed an
    /// executable and not a dynamic library.
    ///
    /// # Note
    /// * When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load as an executable
    /// * `lazy_bind` - Optional override for lazy binding behavior
    ///
    /// # Returns
    /// * `Ok(ElfExec)` - The loaded executable
    /// * `Err(Error)` - If loading fails
    pub fn load_exec(
        &mut self,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
    ) -> Result<ElfExec> {
        // Prepare and validate the ELF header
        let ehdr = self.buf.prepare_ehdr(&mut object)?;

        // Ensure the file is actually an executable and not a dynamic library
        if ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }

        // Load the relocated common part
        let inner = self.load_relocated(ehdr, object, lazy_bind)?;

        // Wrap in ElfExec and return
        Ok(ElfExec { inner })
    }
}

/// An executable file that has been relocated
///
/// This structure represents an executable ELF file that has been loaded
/// and relocated in memory, making it ready for execution. It contains
/// the entry point and other information needed to run the executable.
#[derive(Clone)]
pub struct RelocatedExec<'scope> {
    /// Entry point of the executable
    entry: usize,

    /// The relocated ELF object
    inner: Relocated<'scope>,
}

impl RelocatedExec<'_> {
    /// Gets the entry point of the executable
    ///
    /// # Returns
    /// The virtual address of the entry point
    #[inline]
    pub fn entry(&self) -> usize {
        self.entry
    }
}

impl Debug for RelocatedExec<'_> {
    /// Formats the RelocatedExec for debugging purposes
    ///
    /// This implementation delegates to the inner Relocated object's
    /// debug implementation.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

impl Deref for RelocatedExec<'_> {
    type Target = CoreComponent;

    /// Dereferences to the underlying CoreComponent
    ///
    /// This implementation allows direct access to the CoreComponent
    /// fields through the RelocatedExec wrapper.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
