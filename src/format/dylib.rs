use crate::{
    CoreComponent, Loader, Result, UserData,
    arch::{ElfPhdr, ElfRela, Phdr},
    dynamic::ElfDynamic,
    loader::Builder,
    mmap::Mmap,
    object::{ElfObject, ElfObjectAsync},
    parse_ehdr_error,
    relocation::{RelocateHelper, relocate_impl},
    segment::ElfSegments,
    symbol::{SymbolInfo, SymbolTable},
};
use alloc::{boxed::Box, ffi::CString, sync::Arc};
use core::{any::Any, ffi::CStr, fmt::Debug, marker::PhantomData, ops::Deref};

use super::{ElfCommonPart, Relocated};

/// An unrelocated dynamic library
pub struct ElfDylib {
    common: ElfCommonPart,
}

impl Deref for ElfDylib {
    type Target = ElfCommonPart;

    fn deref(&self) -> &Self::Target {
        &self.common
    }
}

impl Debug for ElfDylib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ElfLibrary")
            .field("name", &self.common.inner.name)
            .field("needed_libs", &self.common.inner.needed_libs)
            .finish()
    }
}

impl ElfDylib {
    /// Gets mutable user data from the elf object.
    #[inline]
    pub fn user_data_mut(&mut self) -> Option<&mut UserData> {
        Arc::get_mut(&mut self.common.core.inner).map(|inner| &mut inner.user_data)
    }

    /// Relocate the dynamic library with the given dynamic libraries and function closure.
    /// # Note
    /// During relocation, the symbol is first searched in the function closure `pre_find`.
    pub fn easy_relocate<'iter, 'scope, 'find, 'lib, S, F>(
        self,
        scope: S,
        pre_find: &'find F,
    ) -> Result<RelocatedDylib<'lib>>
    where
        S: Iterator<Item = &'iter RelocatedDylib<'scope>> + Clone,
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        self.relocate(scope, pre_find, |_, _, _| Err(Box::new(())), None)
    }

    /// Relocate the dynamic library with the given dynamic libraries and function closure.
    /// # Note
    /// * During relocation, the symbol is first searched in the function closure `pre_find`.
    /// * The `deal_unknown` function is used to handle relocation types not implemented by efl_loader or failed relocations
    /// * Typically, the `scope` should also contain the current dynamic library itself,
    /// relocation will be done in the exact order in which the dynamic libraries appear in `scope`.
    /// * When lazy binding, the symbol is first looked for in the global scope and then in the local lazy scope
    pub fn relocate<'iter, 'scope, 'find, 'lib, S, F, D>(
        self,
        scope: S,
        pre_find: &'find F,
        deal_unknown: D,
        local_lazy_scope: Option<Box<dyn for<'a> Fn(&'a str) -> Option<*const ()> + 'static>>,
    ) -> Result<RelocatedDylib<'lib>>
    where
        S: Iterator<Item = &'iter RelocatedDylib<'scope>> + Clone,
        F: Fn(&str) -> Option<*const ()>,
        D: Fn(&ElfRela, &CoreComponent, S) -> core::result::Result<(), Box<dyn Any>>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        let helper = scope
            .clone()
            .map(|lib| RelocateHelper {
                base: lib.base(),
                symtab: lib.symtab(),
            })
            .collect();
        let scope_clone = scope.clone();
        let wrapper =
            |rela: &ElfRela, core: &CoreComponent| deal_unknown(rela, core, scope_clone.clone());
        Ok(RelocatedDylib {
            core: relocate_impl(self.common, helper, pre_find, wrapper, local_lazy_scope)?,
        })
    }
}

impl Builder {
    pub(crate) fn create_dylib(self, phdrs: &[ElfPhdr]) -> Result<ElfDylib> {
        let common = self.create_common(phdrs, true)?;
        Ok(ElfDylib { common })
    }
}

impl<M: Mmap> Loader<M> {
    /// Load a dynamic library into memory
    pub fn easy_load_dylib(&mut self, object: impl ElfObject) -> Result<ElfDylib> {
        self.load_dylib(object, None, |_, _, _, _| Ok(()))
    }

    /// Load a dynamic library into memory
    /// # Note
    /// * `hook` functions are called first when a program header is processed.
    /// * When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    pub fn load_dylib<F>(
        &mut self,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
        hook: F,
    ) -> Result<ElfDylib>
    where
        F: Fn(&CStr, &Phdr, &ElfSegments, &mut UserData) -> core::result::Result<(), Box<dyn Any>>,
    {
        let ehdr = self.read_ehdr(&mut object)?;
        if !ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }
        self.load_impl(ehdr, object, lazy_bind, hook, |builder, phdrs| {
            builder.create_dylib(phdrs)
        })
    }

    /// Load a dynamic library into memory
    /// # Note
    /// `hook` functions are called first when a program header is processed.
    pub async fn load_dylib_async<F>(
        &mut self,
        mut object: impl ElfObjectAsync,
        lazy_bind: Option<bool>,
        hook: F,
    ) -> Result<ElfDylib>
    where
        F: Fn(&CStr, &Phdr, &ElfSegments, &mut UserData) -> core::result::Result<(), Box<dyn Any>>,
    {
        let ehdr = self.read_ehdr(&mut object)?;
		if !ehdr.is_dylib() {
            return Err(parse_ehdr_error("file type mismatch"));
        }
        self.load_async_impl(ehdr, object, lazy_bind, hook, |builder, phdrs| {
            builder.create_dylib(phdrs)
        })
        .await
    }
}

/// A dynamic library that has been relocated
#[derive(Clone)]
pub struct RelocatedDylib<'scope> {
    core: Relocated<'scope>,
}

impl Debug for RelocatedDylib<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.core.fmt(f)
    }
}

impl Deref for RelocatedDylib<'_> {
    type Target = CoreComponent;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl RelocatedDylib<'_> {
    /// # Safety
    /// The current elf object has not yet been relocated, so it is dangerous to use this
    /// function to convert `CoreComponent` to `RelocateDylib`. And lifecycle information is lost
    #[inline]
    pub unsafe fn from_core_component(core: CoreComponent) -> Self {
        RelocatedDylib {
            core: Relocated {
                core,
                _marker: PhantomData,
            },
        }
    }

    /// Gets the core component reference of the elf object.
    /// # Safety
    /// Lifecycle information is lost, and the dependencies of the current elf object can be prematurely deallocated,
    /// which can cause serious problems.
    #[inline]
    pub unsafe fn core_component_ref(&self) -> &CoreComponent {
        &self.core
    }

    #[inline]
    pub unsafe fn new_uncheck(
        name: CString,
        base: usize,
        dynamic: ElfDynamic,
        phdrs: &'static [ElfPhdr],
        segments: ElfSegments,
        user_data: UserData,
    ) -> Self {
        Self {
            core: Relocated {
                core: CoreComponent::from_raw(name, base, dynamic, phdrs, segments, user_data),
                _marker: PhantomData,
            },
        }
    }

    /// Gets the symbol table.
    #[inline]
    pub fn symtab(&self) -> &SymbolTable {
        self.core.symtab().unwrap()
    }

    /// Gets a pointer to a function or static variable by symbol name.
    ///
    /// The symbol is interpreted as-is; no mangling is done. This means that symbols like `x::y` are
    /// most likely invalid.
    ///
    /// # Safety
    /// Users of this API must specify the correct type of the function or variable loaded.
    ///
    /// # Examples
    /// ```no_run
    /// # use elf_loader::{object::ElfFile, Symbol, mmap::MmapImpl, Loader};
    /// # let mut loader = Loader::<MmapImpl>::new();
    /// # let lib = loader
    /// #     .easy_load_dylib(ElfFile::from_path("target/liba.so").unwrap())
    /// #        .unwrap().easy_relocate([].iter(), &|_|{None}).unwrap();;
    /// unsafe {
    ///     let awesome_function: Symbol<unsafe extern fn(f64) -> f64> =
    ///         lib.get("awesome_function").unwrap();
    ///     awesome_function(0.42);
    /// }
    /// ```
    /// A static variable may also be loaded and inspected:
    /// ```no_run
    /// # use elf_loader::{object::ElfFile, Symbol, mmap::MmapImpl, Loader};
    /// # let mut loader = Loader::<MmapImpl>::new();
    /// # let lib = loader
    /// #     .easy_load_dylib(ElfFile::from_path("target/liba.so").unwrap())
    /// #        .unwrap().easy_relocate([].iter(), &|_|{None}).unwrap();;
    /// unsafe {
    ///     let awesome_variable: Symbol<*mut f64> = lib.get("awesome_variable").unwrap();
    ///     **awesome_variable = 42.0;
    /// };
    /// ```
    #[inline]
    pub unsafe fn get<'lib, T>(&'lib self, name: &str) -> Option<Symbol<'lib, T>> {
        self.symtab()
            .lookup_filter(&SymbolInfo::from_str(name))
            .map(|sym| Symbol {
                ptr: (self.base() + sym.st_value()) as _,
                pd: PhantomData,
            })
    }

    /// Load a versioned symbol from the elf object.
    ///
    /// # Examples
    /// ```
    /// let symbol = unsafe { lib.get_version::<fn()>>("function_name", "1.0").unwrap() };
    /// ```
    #[cfg(feature = "version")]
    #[inline]
    pub unsafe fn get_version<'lib, T>(
        &'lib self,
        name: &str,
        version: &str,
    ) -> Option<Symbol<'lib, T>> {
        self.symtab()
            .lookup_filter(&SymbolInfo::new_with_version(name, version))
            .map(|sym| Symbol {
                ptr: (self.base() + sym.st_value()) as _,
                pd: PhantomData,
            })
    }
}

/// A symbol from elf object
#[derive(Debug, Clone)]
pub struct Symbol<'lib, T: 'lib> {
    ptr: *mut (),
    pd: PhantomData<&'lib T>,
}

impl<'lib, T> Deref for Symbol<'lib, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*(&self.ptr as *const *mut _ as *const T) }
    }
}

impl<'lib, T> Symbol<'lib, T> {
    pub fn into_raw(self) -> *const () {
        self.ptr
    }
}
