use super::{ElfCommonPart, Relocated, create_lazy_scope};
use crate::{
    CoreComponent, Loader, Result, UserData,
    arch::ElfPhdr,
    dynamic::ElfDynamic,
    loader::Builder,
    mmap::Mmap,
    object::{ElfObject, ElfObjectAsync},
    parse_ehdr_error,
    relocation::{LazyScope, SymDef, UnknownHandler, relocate_impl},
    segment::ElfSegments,
    symbol::{SymbolInfo, SymbolTable},
};
use alloc::{boxed::Box, ffi::CString, vec::Vec};
use core::{fmt::Debug, marker::PhantomData, ops::Deref};

/// An unrelocated dynamic library
pub struct ElfDylib {
    inner: ElfCommonPart,
}

impl Deref for ElfDylib {
    type Target = ElfCommonPart;

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
        scope: impl IntoIterator<Item = &'iter RelocatedDylib<'scope>>,
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
        scope: impl AsRef<[&'iter RelocatedDylib<'scope>]>,
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
        Ok(RelocatedDylib {
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

impl Builder {
    pub(crate) fn create_dylib(self, phdrs: &[ElfPhdr]) -> ElfDylib {
        let inner = self.create_inner(phdrs, true);
        ElfDylib { inner }
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
        let (builder, phdrs) = self.load_impl(ehdr, object, lazy_bind)?;
        Ok(builder.create_dylib(phdrs))
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
        let (builder, phdrs) = self.load_async_impl(ehdr, object, lazy_bind).await?;
        Ok(builder.create_dylib(phdrs))
    }
}

/// A dynamic library that has been relocated
#[derive(Clone)]
pub struct RelocatedDylib<'scope> {
    inner: Relocated<'scope>,
}

impl Debug for RelocatedDylib<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

impl Deref for RelocatedDylib<'_> {
    type Target = CoreComponent;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl RelocatedDylib<'_> {
    /// # Safety
    /// The current elf object has not yet been relocated, so it is dangerous to use this
    /// function to convert `CoreComponent` to `RelocateDylib`. And lifecycle information is lost
    #[inline]
    pub unsafe fn from_core_component(core: CoreComponent) -> Self {
        RelocatedDylib {
            inner: Relocated {
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
        &self.inner
    }

    /// # Safety
    /// The caller needs to ensure that the parameters passed in come from a valid dynamic library.
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
            inner: Relocated {
                core: CoreComponent::from_raw(name, base, dynamic, phdrs, segments, user_data),
                _marker: PhantomData,
            },
        }
    }

    /// Gets the symbol table.
    #[inline]
    pub fn symtab(&self) -> &SymbolTable {
        unsafe { self.inner.symtab().unwrap_unchecked() }
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
    /// # use elf_loader::{object::ElfBinary, Symbol, mmap::MmapImpl, Loader};
    /// # let mut loader = Loader::<MmapImpl>::new();
    /// # let lib = loader
    /// #     .easy_load_dylib(ElfBinary::new("target/liba.so", &[]))
    /// #        .unwrap().easy_relocate([].iter(), &|_|{None}).unwrap();
    /// unsafe {
    ///     let awesome_function: Symbol<unsafe extern fn(f64) -> f64> =
    ///         lib.get("awesome_function").unwrap();
    ///     awesome_function(0.42);
    /// }
    /// ```
    /// A static variable may also be loaded and inspected:
    /// ```no_run
    /// # use elf_loader::{object::ElfBinary, Symbol, mmap::MmapImpl, Loader};
    /// # let mut loader = Loader::<MmapImpl>::new();
    /// # let lib = loader
    /// #     .easy_load_dylib(ElfBinary::new("target/liba.so", &[]))
    /// #        .unwrap().easy_relocate([].iter(), &|_|{None}).unwrap();
    /// unsafe {
    ///     let awesome_variable: Symbol<*mut f64> = lib.get("awesome_variable").unwrap();
    ///     **awesome_variable = 42.0;
    /// };
    /// ```
    #[inline]
    pub unsafe fn get<'lib, T>(&'lib self, name: &str) -> Option<Symbol<'lib, T>> {
        let syminfo = SymbolInfo::from_str(name, None);
        let mut precompute = syminfo.precompute();
        self.symtab()
            .lookup_filter(&syminfo, &mut precompute)
            .map(|sym| Symbol {
                ptr: SymDef {
                    sym: Some(sym),
                    lib: self,
                }
                .convert() as _,
                pd: PhantomData,
            })
    }

    /// Load a versioned symbol from the elf object.
    /// # Safety
    /// Users of this API must specify the correct type of the function or variable loaded.
    /// # Examples
    /// ```no_run
    /// # use elf_loader::{object::ElfFile, Symbol, mmap::MmapImpl, Loader};
    /// # let mut loader = Loader::<MmapImpl>::new();
    /// # let lib = loader
    /// #     .easy_load_dylib(ElfFile::from_path("target/liba.so").unwrap())
    /// #        .unwrap().easy_relocate([].iter(), &|_|{None}).unwrap();;
    /// let symbol = unsafe { lib.get_version::<fn()>("function_name", "1.0").unwrap() };
    /// ```
    #[cfg(feature = "version")]
    #[inline]
    pub unsafe fn get_version<'lib, T>(
        &'lib self,
        name: &str,
        version: &str,
    ) -> Option<Symbol<'lib, T>> {
        let syminfo = SymbolInfo::from_str(name, Some(version));
        let mut precompute = syminfo.precompute();
        self.symtab()
            .lookup_filter(&syminfo, &mut precompute)
            .map(|sym| Symbol {
                ptr: SymDef {
                    sym: Some(sym),
                    lib: self,
                }
                .convert() as _,
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
