use super::{ElfCommonPart, Relocated};
use crate::{
    CoreComponent, Loader, RelocatedDylib, Result, UserData,
    arch::{ElfPhdr, ElfRela},
    loader::Builder,
    mmap::Mmap,
    object::{ElfObject, ElfObjectAsync},
    parse_ehdr_error,
    relocation::{RelocateHelper, relocate_impl},
    segment::ElfSegments,
};
use alloc::{boxed::Box, vec::Vec};
use core::{any::Any, ffi::CStr, fmt::Debug, marker::PhantomData, ops::Deref};

/// An unrelocated executable file
pub struct ElfExec {
    common: ElfCommonPart,
}

impl Deref for ElfExec {
    type Target = ElfCommonPart;

    fn deref(&self) -> &Self::Target {
        &self.common
    }
}

impl Debug for ElfExec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ElfExec")
            .field("name", &self.core.inner.name)
            .field("needed_libs", &self.core.inner.needed_libs)
            .finish()
    }
}

impl ElfExec {
    /// Relocate the executable file with the given dynamic libraries and function closure.
    /// # Note
    /// During relocation, the symbol is first searched in the function closure `pre_find`.
    pub fn easy_relocate<'iter, 'scope, 'find, 'lib, S, F>(
        self,
        scope: S,
        pre_find: &'find F,
    ) -> Result<RelocatedExec<'lib>>
    where
        S: Iterator<Item = &'iter RelocatedDylib<'scope>> + Clone,
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        self.relocate(scope, pre_find, |_, _, _| Err(Box::new(())), None)
    }

    /// Relocate the executable file with the given dynamic libraries and function closure.
    /// # Note
    /// * During relocation, the symbol is first searched in the function closure `pre_find`.
    /// * The `deal_unknown` function is used to handle relocation types not implemented by efl_loader or failed relocations
    /// * relocation will be done in the exact order in which the dynamic libraries appear in `scope`.
    /// * When lazy binding, the symbol is first looked for in the global scope and then in the local lazy scope
    pub fn relocate<'iter, 'scope, 'find, 'lib, S, F, D>(
        self,
        scope: S,
        pre_find: &'find F,
        deal_unknown: D,
        local_lazy_scope: Option<Box<dyn for<'a> Fn(&'a str) -> Option<*const ()> + 'static>>,
    ) -> Result<RelocatedExec<'lib>>
    where
        S: Iterator<Item = &'iter RelocatedDylib<'scope>> + Clone,
        F: Fn(&str) -> Option<*const ()>,
        D: Fn(&ElfRela, &CoreComponent, S) -> core::result::Result<(), Box<dyn Any>>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        if self.relocation.is_empty() {
            return Ok(RelocatedExec {
                entry: self.entry,
                core: Relocated {
                    core: self.common.core,
                    _marker: PhantomData,
                },
            });
        }
        let mut helper = Vec::new();
        if let Some(symtab) = self.symtab() {
            helper.push(unsafe {
                core::mem::transmute(RelocateHelper {
                    base: self.base(),
                    symtab,
                    #[cfg(feature = "log")]
                    lib_name: self.name(),
                })
            });
        }
        scope.clone().for_each(|lib| {
            helper.push(RelocateHelper {
                base: lib.base(),
                symtab: lib.symtab(),
                #[cfg(feature = "log")]
                lib_name: lib.name(),
            })
        });
        let scope_clone = scope.clone();
        let wrapper =
            |rela: &ElfRela, core: &CoreComponent| deal_unknown(rela, core, scope_clone.clone());
        Ok(RelocatedExec {
            entry: self.entry,
            core: relocate_impl(self.common, helper, pre_find, wrapper, local_lazy_scope)?,
        })
    }
}

impl Builder {
    pub(crate) fn create_exec(self, _phdrs: &[ElfPhdr]) -> Result<ElfExec> {
        let common = self.create_common(&[], false)?;
        Ok(ElfExec { common })
    }
}

impl<M: Mmap> Loader<M> {
    /// Load a executable file into memory
    pub fn easy_load_exec(&mut self, object: impl ElfObject) -> Result<ElfExec> {
        self.load_exec(object, None, |_, _, _, _| Ok(()))
    }

    /// Load a executable file into memory
    /// # Note
    /// * `hook` functions are called first when a program header is processed.
    /// * When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    pub fn load_exec<F>(
        &mut self,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
        hook: F,
    ) -> Result<ElfExec>
    where
        F: Fn(
            &CStr,
            &ElfPhdr,
            &ElfSegments,
            &mut UserData,
        ) -> core::result::Result<(), Box<dyn Any>>,
    {
        let ehdr = self.prepare_ehdr(&mut object)?;
        if ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }
        self.load_impl(ehdr, object, lazy_bind, hook, |builder, phdrs| {
            builder.create_exec(phdrs)
        })
    }

    /// Load a executable file into memory
    /// # Note
    /// `hook` functions are called first when a program header is processed.
    pub async fn load_exec_async<F>(
        &mut self,
        mut object: impl ElfObjectAsync,
        lazy_bind: Option<bool>,
        hook: F,
    ) -> Result<ElfExec>
    where
        F: Fn(
            &CStr,
            &ElfPhdr,
            &ElfSegments,
            &mut UserData,
        ) -> core::result::Result<(), Box<dyn Any>>,
    {
        let ehdr = self.prepare_ehdr(&mut object)?;
        if ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }
        self.load_async_impl(ehdr, object, lazy_bind, hook, |builder, phdrs| {
            builder.create_exec(phdrs)
        })
        .await
    }
}

/// A executable file that has been relocated
#[derive(Clone)]
pub struct RelocatedExec<'scope> {
    entry: usize,
    core: Relocated<'scope>,
}

impl RelocatedExec<'_> {
    #[inline]
    pub fn entry(&self) -> usize {
        self.entry
    }
}

impl Debug for RelocatedExec<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.core.fmt(f)
    }
}

impl Deref for RelocatedExec<'_> {
    type Target = CoreComponent;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}
