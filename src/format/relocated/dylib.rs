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
pub struct ElfDylib {
    inner: RelocatedCommonPart,
}

impl Deref for ElfDylib {
    type Target = RelocatedCommonPart;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Debug for ElfDylib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ElfDylib")
            .field("name", &self.inner.name())
            .field("needed_libs", &self.inner.needed_libs())
            .finish()
    }
}

impl ElfDylib {
    /// Gets mutable user data from the elf object.
    #[inline]
    pub fn user_data_mut(&mut self) -> Option<&mut UserData> {
        self.inner.user_data_mut()
    }

    /// Relocate the dynamic library with the given dynamic libraries and function closure.
    /// # Note
    /// During relocation, the symbol is first searched in the function closure `pre_find`.
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

    /// Relocate the dynamic library with the given dynamic libraries and function closure.
    /// # Note
    /// * During relocation, the symbol is first searched in the function closure `pre_find`.
    /// * The `deal_unknown` function is used to handle relocation types not implemented by efl_loader or failed relocations
    /// * Typically, the `scope` should also contain the current dynamic library itself,
    ///   relocation will be done in the exact order in which the dynamic libraries appear in `scope`.
    /// * When lazy binding, the symbol is first looked for in the global scope and then in the local lazy scope
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
    pub fn easy_load_dylib(&mut self, object: impl ElfObject) -> Result<ElfDylib> {
        self.load_dylib(object, None)
    }

    /// Load a dynamic library into memory
    /// # Note
    /// When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    pub fn load_dylib(
        &mut self,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
    ) -> Result<ElfDylib> {
        let ehdr = self.buf.prepare_ehdr(&mut object)?;
        if !ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }
        let inner = self.load_relocated(ehdr, object, lazy_bind)?;
        Ok(ElfDylib { inner })
    }

    /// Load a dynamic library into memory
    /// # Note
    /// When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    pub async fn load_dylib_async(
        &mut self,
        mut object: impl ElfObjectAsync,
        lazy_bind: Option<bool>,
    ) -> Result<ElfDylib> {
        let ehdr = self.buf.prepare_ehdr(&mut object)?;
        if !ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }
        let inner = self.load_relocated_async(ehdr, object, lazy_bind).await?;
        Ok(ElfDylib { inner })
    }
}

pub type RelocatedDylib<'lib> = Relocated<'lib>;
