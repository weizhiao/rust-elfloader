use crate::{
    CoreComponent, Error, Result,
    arch::{ElfRelType, ElfSymbol},
    format::Relocated,
    relocate_error,
    relocation::dynamic_link::LazyScope,
    symbol::{SymbolInfo, SymbolTable},
};
use alloc::{format, string::ToString, vec::Vec};
use core::{
    ops::{Add, Sub},
    ptr::null,
};
use elf::abi::STT_GNU_IFUNC;

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

pub(crate) mod dynamic_link;
pub(crate) mod static_link;

/// A trait for looking up symbols during relocation
///
/// This trait allows for flexible symbol resolution strategies, supporting
/// both closures and complex structs with state.
pub trait SymbolLookup {
    /// Find the address of a symbol by name
    fn lookup(&self, name: &str) -> Option<*const ()>;
}

impl<F> SymbolLookup for F
where
    F: Fn(&str) -> Option<*const ()> + ?Sized,
{
    fn lookup(&self, name: &str) -> Option<*const ()> {
        self(name)
    }
}

impl<S: SymbolLookup + ?Sized> SymbolLookup for Arc<S> {
    fn lookup(&self, name: &str) -> Option<*const ()> {
        (**self).lookup(name)
    }
}

impl SymbolLookup for () {
    fn lookup(&self, _name: &str) -> Option<*const ()> {
        None
    }
}

/// A trait for handling unknown relocations
pub trait RelocationHandler {
    /// Handle an unknown relocation
    ///
    /// Returns:
    /// - `Some(Ok(None))`: Handled successfully
    /// - `Some(Ok(Some(idx)))`: Handled successfully and the library at `scope[idx]` is used
    /// - `Some(Err(e))`: Handled but failed
    /// - `None`: Not handled (fallthrough)
    fn handle(
        &mut self,
        rel: &ElfRelType,
        lib: &CoreComponent,
        scope: &[Relocated],
    ) -> Option<core::result::Result<Option<usize>, Error>>;
}

impl<F> RelocationHandler for F
where
    F: FnMut(
            &ElfRelType,
            &CoreComponent,
            &[Relocated],
        ) -> Option<core::result::Result<Option<usize>, Error>>
        + ?Sized,
{
    fn handle(
        &mut self,
        rel: &ElfRelType,
        lib: &CoreComponent,
        scope: &[Relocated],
    ) -> Option<core::result::Result<Option<usize>, Error>> {
        self(rel, lib, scope)
    }
}

impl RelocationHandler for () {
    fn handle(
        &mut self,
        _rel: &ElfRelType,
        _lib: &CoreComponent,
        _scope: &[Relocated],
    ) -> Option<core::result::Result<Option<usize>, Error>> {
        None
    }
}

/// A trait for objects that can be relocated
pub trait Relocatable: Sized {
    /// The type of the relocated object
    type Output;

    /// Create a builder for relocating the dynamic library
    ///
    /// This method returns a `Relocator` that allows configuring the relocation
    /// process with fine-grained control, such as setting a custom unknown relocation
    /// handler, forcing lazy/eager binding, and specifying the symbol resolution scope.
    fn relocator(self) -> Relocator<'static, Self, (), (), ()> {
        Relocator::new(self)
    }

    /// Execute the relocation process
    #[doc(hidden)]
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
        PostH: RelocationHandler;
}

/// A builder for configuring and executing the relocation process
pub struct Relocator<'find, T, S, PreH, PostH>
where
    T: Relocatable,
    S: SymbolLookup,
    PreH: RelocationHandler,
    PostH: RelocationHandler,
{
    object: T,
    scope: Vec<Relocated>,
    pre_find: S,
    pre_handler: PreH,
    post_handler: PostH,
    lazy: Option<bool>,
    lazy_scope: Option<LazyScope>,
    use_scope_as_lazy: bool,
    _marker: core::marker::PhantomData<&'find ()>,
}

impl<'find, T: Relocatable> Relocator<'find, T, (), (), ()> {
    /// Create a new relocator builder
    pub fn new(object: T) -> Self {
        Self {
            object,
            scope: Vec::new(),
            pre_find: (),
            pre_handler: (),
            post_handler: (),
            lazy: None,
            lazy_scope: None,
            use_scope_as_lazy: false,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<'find, T, S, PreH, PostH> Relocator<'find, T, S, PreH, PostH>
where
    T: Relocatable,
    S: SymbolLookup,
    PreH: RelocationHandler,
    PostH: RelocationHandler,
{
    /// Set the preferred symbol lookup function
    pub fn symbols<S2: SymbolLookup + 'find>(
        self,
        pre_find: S2,
    ) -> Relocator<'find, T, S2, PreH, PostH> {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find,
            pre_handler: self.pre_handler,
            post_handler: self.post_handler,
            lazy: self.lazy,
            lazy_scope: self.lazy_scope,
            use_scope_as_lazy: self.use_scope_as_lazy,
            _marker: core::marker::PhantomData,
        }
    }

    /// Set the scope of relocated libraries for symbol resolution
    pub fn scope<I, R>(mut self, scope: I) -> Self
    where
        I: IntoIterator<Item = R>,
        R: core::borrow::Borrow<Relocated>,
    {
        self.scope = scope.into_iter().map(|r| r.borrow().clone()).collect();
        self
    }

    /// Set the pre-processing relocation handler (pre_handler)
    pub fn pre_handler<NewPreH: RelocationHandler + 'find>(
        self,
        handler: NewPreH,
    ) -> Relocator<'find, T, S, NewPreH, PostH> {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find: self.pre_find,
            pre_handler: handler,
            post_handler: self.post_handler,
            lazy: self.lazy,
            lazy_scope: self.lazy_scope,
            use_scope_as_lazy: self.use_scope_as_lazy,
            _marker: core::marker::PhantomData,
        }
    }

    /// Set the post-processing relocation handler (post_handler)
    pub fn post_handler<NewPostH: RelocationHandler + 'find>(
        self,
        handler: NewPostH,
    ) -> Relocator<'find, T, S, PreH, NewPostH> {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find: self.pre_find,
            pre_handler: self.pre_handler,
            post_handler: handler,
            lazy: self.lazy,
            lazy_scope: self.lazy_scope,
            use_scope_as_lazy: self.use_scope_as_lazy,
            _marker: core::marker::PhantomData,
        }
    }

    /// Enable or disable lazy binding
    pub fn lazy(mut self, lazy: bool) -> Self {
        self.lazy = Some(lazy);
        self
    }

    /// Set the lazy scope (symbol lookup for lazy binding)
    pub fn lazy_scope(self, scope: Option<LazyScope>) -> Relocator<'find, T, S, PreH, PostH> {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find: self.pre_find,
            pre_handler: self.pre_handler,
            post_handler: self.post_handler,
            lazy: self.lazy,
            lazy_scope: scope,
            use_scope_as_lazy: self.use_scope_as_lazy,
            _marker: core::marker::PhantomData,
        }
    }

    /// Use scope as lazy scope (overrides any previously set lazy scope)
    pub fn use_scope_as_lazy(mut self) -> Self {
        self.use_scope_as_lazy = true;
        self
    }

    /// Execute the relocation process
    pub fn relocate(self) -> Result<T::Output> {
        self.object.relocate(
            &self.scope,
            &self.pre_find,
            self.pre_handler,
            self.post_handler,
            self.lazy,
            self.lazy_scope,
            self.use_scope_as_lazy,
        )
    }
}

/// A trait for handling unknown relocations

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub(crate) struct RelocValue<T>(pub T);

impl<T> RelocValue<T> {
    #[inline]
    pub const fn new(val: T) -> Self {
        Self(val)
    }
}

impl RelocValue<usize> {
    #[inline]
    #[allow(dead_code)]
    pub const fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }

    #[inline]
    pub const fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }
}

impl Add<usize> for RelocValue<usize> {
    type Output = Self;

    #[inline]
    fn add(self, rhs: usize) -> Self::Output {
        RelocValue(self.0.wrapping_add(rhs))
    }
}

impl Add<isize> for RelocValue<usize> {
    type Output = Self;

    #[inline]
    fn add(self, rhs: isize) -> Self::Output {
        RelocValue(self.0.wrapping_add_signed(rhs))
    }
}

impl Sub<usize> for RelocValue<usize> {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: usize) -> Self::Output {
        RelocValue(self.0.wrapping_sub(rhs))
    }
}

impl From<usize> for RelocValue<usize> {
    #[inline]
    fn from(val: usize) -> Self {
        Self(val)
    }
}

impl From<RelocValue<usize>> for usize {
    #[inline]
    fn from(value: RelocValue<usize>) -> Self {
        value.0
    }
}

impl TryFrom<RelocValue<usize>> for RelocValue<i32> {
    type Error = crate::Error;

    #[inline]
    fn try_from(value: RelocValue<usize>) -> Result<Self> {
        i32::try_from(value.0 as isize)
            .map(RelocValue)
            .map_err(|err| relocate_error(err.to_string()))
    }
}

impl TryFrom<RelocValue<usize>> for RelocValue<u32> {
    type Error = crate::Error;

    #[inline]
    fn try_from(value: RelocValue<usize>) -> Result<Self> {
        u32::try_from(value.0)
            .map(RelocValue)
            .map_err(|err| relocate_error(err.to_string()))
    }
}

pub struct SymDef<'lib> {
    pub sym: Option<&'lib ElfSymbol>,
    pub lib: &'lib CoreComponent,
}

impl<'temp> SymDef<'temp> {
    // 获取符号的真实地址(base + st_value)
    pub fn convert(self) -> *const () {
        if likely(self.sym.is_some()) {
            let base = self.lib.base();
            let sym = unsafe { self.sym.unwrap_unchecked() };
            if likely(sym.st_type() != STT_GNU_IFUNC) {
                (base + sym.st_value()) as _
            } else {
                // IFUNC会在运行时确定地址，这里使用的是ifunc的返回值
                let ifunc: fn() -> usize = unsafe { core::mem::transmute(base + sym.st_value()) };
                ifunc() as _
            }
        } else {
            // 未定义的弱符号返回null
            null()
        }
    }
}

#[cold]
pub(crate) fn reloc_error<E: core::fmt::Display>(
    r_type: usize,
    r_sym: usize,
    err: E,
    lib: &CoreComponent,
) -> Error {
    if r_sym == 0 {
        relocate_error(format!(
            "file: {}, relocation type: {}, no symbol, error: {}",
            lib.shortname(),
            r_type,
            err
        ))
    } else {
        relocate_error(format!(
            "file: {}, relocation type: {}, symbol name: {}, error: {}",
            lib.shortname(),
            r_type,
            lib.symtab().unwrap().symbol_idx(r_sym).1.name(),
            err
        ))
    }
}

fn find_weak<'lib>(lib: &'lib CoreComponent, dynsym: &'lib ElfSymbol) -> Option<SymDef<'lib>> {
    // 弱符号 + WEAK 用 0 填充rela offset
    if dynsym.is_weak() && dynsym.is_undef() {
        assert!(dynsym.st_value() == 0);
        Some(SymDef { sym: None, lib })
    } else if dynsym.st_value() != 0 {
        Some(SymDef {
            sym: Some(dynsym),
            lib,
        })
    } else {
        None
    }
}

/// Find symbol definition in the given scope
/// This function searches for the definition of a symbol
/// identified by `r_sym` in the provided `libs` scope.
/// It returns a tuple containing the symbol definition
/// and an optional index of the library where it was found.
pub fn find_symdef<'lib>(
    core: &'lib CoreComponent,
    libs: &'lib [Relocated],
    r_sym: usize,
) -> Option<(SymDef<'lib>, Option<usize>)> {
    let symbol = core.symtab().unwrap();
    let (sym, syminfo) = symbol.symbol_idx(r_sym);
    find_symdef_impl(core, libs, sym, &syminfo)
}

#[inline]
pub(crate) fn find_symbol_addr<S>(
    pre_find: &S,
    core: &CoreComponent,
    symtab: &SymbolTable,
    scope: &[Relocated],
    r_sym: usize,
) -> Option<(RelocValue<usize>, Option<usize>)>
where
    S: SymbolLookup + ?Sized,
{
    let (dynsym, syminfo) = symtab.symbol_idx(r_sym);
    if let Some(addr) = pre_find.lookup(syminfo.name()) {
        #[cfg(feature = "log")]
        log::trace!(
            "binding file [{}] to [pre_find]: symbol [{}]",
            core.name(),
            syminfo.name()
        );
        return Some((RelocValue::new(addr as usize), None));
    }
    find_symdef_impl(core, scope, dynsym, &syminfo)
        .map(|(symdef, idx)| (RelocValue::new(symdef.convert() as usize), idx))
}

fn find_symdef_impl<'lib>(
    core: &'lib CoreComponent,
    libs: &'lib [Relocated],
    sym: &'lib ElfSymbol,
    syminfo: &SymbolInfo,
) -> Option<(SymDef<'lib>, Option<usize>)> {
    if unlikely(sym.is_local()) {
        Some((
            SymDef {
                sym: Some(sym),
                lib: core,
            },
            None,
        ))
    } else {
        let mut precompute = syminfo.precompute();
        libs.iter()
            .enumerate()
            .find_map(|(i, lib)| {
                lib.symtab()
                    .lookup_filter(syminfo, &mut precompute)
                    .map(|sym| {
                        #[cfg(feature = "log")]
                        log::trace!(
                            "binding file [{}] to [{}]: symbol [{}]",
                            core.name(),
                            lib.name(),
                            syminfo.name()
                        );
                        // 如果找到的库和当前 core 指向同一个 ELF（同一 allocation），
                        // 不返回库索引，避免增加引用或产生生命周期循环导致内存泄漏。
                        let same = Arc::as_ptr(&lib.core.inner) == Arc::as_ptr(&core.inner);
                        (
                            SymDef {
                                sym: Some(sym),
                                lib: &lib.core,
                            },
                            if same { None } else { Some(i) },
                        )
                    })
            })
            .or_else(|| find_weak(core, sym).map(|s| (s, None)))
    }
}

#[inline]
#[cold]
fn cold() {}

#[inline]
fn likely(b: bool) -> bool {
    if !b {
        cold()
    }
    b
}

#[inline]
fn unlikely(b: bool) -> bool {
    if b {
        cold()
    }
    b
}
