use super::RelocatedCommonPart;
use crate::{
    CoreComponent, Loader, RelocatedDylib, Result,
    format::{Relocated, create_lazy_scope},
    mmap::Mmap,
    object::{ElfObject, ElfObjectAsync},
    parse_ehdr_error,
    relocation::dynamic_link::{LazyScope, UnknownHandler, relocate_impl},
};
use alloc::{boxed::Box, vec::Vec};
use core::{fmt::Debug, marker::PhantomData, ops::Deref};

impl Deref for ElfExec {
    type Target = RelocatedCommonPart;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// An unrelocated executable file
pub struct ElfExec {
    inner: RelocatedCommonPart,
}

impl Debug for ElfExec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ElfExec")
            .field("name", &self.inner.name())
            .field("needed_libs", &self.inner.needed_libs())
            .finish()
    }
}

impl ElfExec {
    /// Relocate the executable file with the given dynamic libraries and function closure.
    /// # Note
    /// During relocation, the symbol is first searched in the function closure `pre_find`.
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
        if self.inner.relocation().is_empty() {
            return Ok(RelocatedExec {
                entry: self.inner.entry,
                inner: Relocated {
                    core: self.inner.into_core_component(),
                    _marker: PhantomData,
                },
            });
        }
        let mut helper: Vec<&Relocated<'_>> = Vec::new();
        let temp = unsafe { &RelocatedDylib::from_core_component(self.core_component()) };
        if self.inner.symtab().is_some() {
            helper.push(unsafe { core::mem::transmute::<&RelocatedDylib, &RelocatedDylib>(temp) });
        }
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

    /// Relocate the executable file with the given dynamic libraries and function closure.
    /// # Note
    /// * During relocation, the symbol is first searched in the function closure `pre_find`.
    /// * The `deal_unknown` function is used to handle relocation types not implemented by efl_loader or failed relocations
    /// * relocation will be done in the exact order in which the dynamic libraries appear in `scope`.
    /// * When lazy binding, the symbol is first looked for in the global scope and then in the local lazy scope
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
        if self.inner.relocation().is_empty() {
            return Ok(RelocatedExec {
                entry: self.inner.entry,
                inner: Relocated {
                    core: self.inner.into_core_component(),
                    _marker: PhantomData,
                },
            });
        }
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
    /// Load a executable file into memory
    pub fn easy_load_exec(&mut self, object: impl ElfObject) -> Result<ElfExec> {
        self.load_exec(object, None)
    }

    /// Load a executable file into memory
    /// # Note
    /// * When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    pub fn load_exec(
        &mut self,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
    ) -> Result<ElfExec> {
        let ehdr = self.buf.prepare_ehdr(&mut object)?;
        if ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }
        let inner = self.load_relocated(ehdr, object, lazy_bind)?;
        Ok(ElfExec { inner })
    }

    /// Load a executable file into memory
    /// # Note
    /// * When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    pub async fn load_exec_async(
        &mut self,
        mut object: impl ElfObjectAsync,
        lazy_bind: Option<bool>,
    ) -> Result<ElfExec> {
        let ehdr = self.buf.prepare_ehdr(&mut object)?;
        if ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }
        let inner = self.load_relocated_async(ehdr, object, lazy_bind).await?;
        Ok(ElfExec { inner })
    }
}

/// A executable file that has been relocated
#[derive(Clone)]
pub struct RelocatedExec<'scope> {
    entry: usize,
    inner: Relocated<'scope>,
}

impl RelocatedExec<'_> {
    #[inline]
    pub fn entry(&self) -> usize {
        self.entry
    }
}

impl Debug for RelocatedExec<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

impl Deref for RelocatedExec<'_> {
    type Target = CoreComponent;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
