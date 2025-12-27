use crate::{
    ElfModule, Error, LoadedModule, Result,
    arch::{ElfRelType, ElfSymbol},
    relocate_error,
    symbol::{SymbolInfo, SymbolTable},
};
use alloc::{boxed::Box, format, string::ToString, vec::Vec};
use core::{
    ops::{Add, Sub},
    ptr::null,
};
use elf::abi::STT_GNU_IFUNC;

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

mod dynamic_link;
mod static_link;

pub(crate) use dynamic_link::{DynamicRelocation, dl_fixup};
pub(crate) use static_link::{StaticReloc, StaticRelocation};

/// A trait for looking up symbols during relocation.
///
/// This trait allows for flexible symbol resolution strategies, supporting
/// both closures and complex structs with state. It is used to find the
/// addresses of external symbols that the ELF object depends on.
///
/// # Examples
///
/// Using a closure:
/// ```rust
/// use elf_loader::SymbolLookup;
///
/// let lookup = |name: &str| {
///     if name == "my_func" {
///         Some(0x1234 as *const ())
///     } else {
///         None
///     }
/// };
/// ```
///
/// Using a custom struct:
/// ```rust
/// use elf_loader::SymbolLookup;
/// use std::collections::HashMap;
///
/// struct MyResolver {
///     symbols: HashMap<String, *const ()>,
/// }
///
/// impl SymbolLookup for MyResolver {
///     fn lookup(&self, name: &str) -> Option<*const ()> {
///         self.symbols.get(name).copied()
///     }
/// }
/// ```
pub trait SymbolLookup {
    /// Finds the address of a symbol by its name.
    ///
    /// # Arguments
    /// * `name` - The name of the symbol to look up.
    ///
    /// # Returns
    /// * `Some(*const ())` - The address of the symbol if found.
    /// * `None` - If the symbol was not found.
    fn lookup(&self, name: &str) -> Option<*const ()>;
}

impl<F: ?Sized> SymbolLookup for F
where
    F: Fn(&str) -> Option<*const ()>,
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

/// A trait for handling unknown or custom relocations.
///
/// This trait allows users to provide custom logic for relocations that are
/// not handled by the default relocator or for intercepting relocations.
///
/// # Examples
///
/// ```rust
/// use elf_loader::{RelocationHandler, RelocationContext, Result};
///
/// struct MyHandler;
///
/// impl RelocationHandler for MyHandler {
///     fn handle<D>(&mut self, ctx: &RelocationContext<'_, D>) -> Option<Result<Option<usize>>> {
///         let rel = ctx.rel();
///         // Custom logic to handle specific relocation types or symbols
///         None // Fallthrough to default behavior
///     }
/// }
/// ```
pub trait RelocationHandler {
    /// Handles a relocation.
    ///
    /// # Arguments
    /// * `ctx` - The context containing information about the relocation.
    ///
    /// # Returns
    /// * `Some(Ok(None))` - The relocation was handled successfully.
    /// * `Some(Ok(Some(idx)))` - The relocation was handled successfully, and the library at `scope[idx]` was used.
    /// * `Some(Err(e))` - The relocation was handled but failed with an error.
    /// * `None` - The relocation was not handled by this handler (fallthrough to default behavior).
    fn handle<D>(&mut self, ctx: &RelocationContext<'_, D>) -> Option<Result<Option<usize>>>;
}

impl<H: RelocationHandler + ?Sized> RelocationHandler for &mut H {
    fn handle<D>(&mut self, ctx: &RelocationContext<'_, D>) -> Option<Result<Option<usize>>> {
        (**self).handle(ctx)
    }
}

impl<H: RelocationHandler + ?Sized> RelocationHandler for Box<H> {
    fn handle<D>(&mut self, ctx: &RelocationContext<'_, D>) -> Option<Result<Option<usize>>> {
        (**self).handle(ctx)
    }
}

/// Context for relocation operations
struct RelocContext<'a, 'find, D, PreS: ?Sized, PostS: ?Sized, PreH: ?Sized, PostH: ?Sized> {
    scope: &'a [LoadedModule<D>],
    pre_find: &'find PreS,
    post_find: &'find PostS,
    pre_handler: &'a mut PreH,
    post_handler: &'a mut PostH,
    dependency_flags: Vec<bool>,
}

impl<'a, 'find, D, PreS: ?Sized, PostS: ?Sized, PreH: ?Sized, PostH: ?Sized>
    RelocContext<'a, 'find, D, PreS, PostS, PreH, PostH>
where
    PreH: RelocationHandler,
    PostH: RelocationHandler,
{
    #[inline]
    fn handle_pre(&mut self, hctx: &RelocationContext<'_, D>) -> Result<bool> {
        let opt = self.pre_handler.handle(hctx);
        if let Some(r) = opt {
            if let Some(idx) = r? {
                self.dependency_flags[idx] = true;
            }
            return Ok(false);
        }
        Ok(true)
    }

    #[inline]
    fn handle_post(&mut self, hctx: &RelocationContext<'_, D>) -> Result<bool> {
        let opt = self.post_handler.handle(hctx);
        if let Some(r) = opt {
            if let Some(idx) = r? {
                self.dependency_flags[idx] = true;
            }
            return Ok(false);
        }
        Ok(true)
    }
}

/// Context passed to `RelocationHandler::handle` containing the relocation
/// entry, the library where the relocation appears and the current scope.
pub struct RelocationContext<'a, D> {
    rel: &'a ElfRelType,
    lib: &'a ElfModule<D>,
    scope: &'a [LoadedModule<D>],
}

impl<'a, D> RelocationContext<'a, D> {
    /// Construct a new `RelocationContext`.
    #[inline]
    fn new(rel: &'a ElfRelType, lib: &'a ElfModule<D>, scope: &'a [LoadedModule<D>]) -> Self {
        Self { rel, lib, scope }
    }

    /// Access the relocation entry.
    #[inline]
    pub fn rel(&self) -> &ElfRelType {
        self.rel
    }

    /// Access the core component where the relocation appears.
    #[inline]
    pub fn lib(&self) -> &ElfModule<D> {
        self.lib
    }

    /// Access the current resolution scope.
    #[inline]
    pub fn scope(&self) -> &[LoadedModule<D>] {
        self.scope
    }

    /// Find symbol definition in the current scope
    #[inline]
    pub fn find_symdef(&self, r_sym: usize) -> Option<(SymDef<'a, D>, Option<usize>)> {
        let symbol = self.lib.symtab();
        let (sym, syminfo) = symbol.symbol_idx(r_sym);
        find_symdef_impl(self.lib, self.scope, sym, &syminfo)
    }
}

impl RelocationHandler for () {
    fn handle<D>(&mut self, _ctx: &RelocationContext<'_, D>) -> Option<Result<Option<usize>>> {
        None
    }
}

/// A trait for objects that can be relocated
pub trait Relocatable<D = ()>: Sized {
    /// The type of the relocated object
    type Output;

    /// Execute the relocation process
    fn relocate<PreS, PostS, LazyS, PreH, PostH>(
        self,
        scope: &[LoadedModule<D>],
        pre_find: &PreS,
        post_find: &PostS,
        pre_handler: PreH,
        post_handler: PostH,
        lazy: Option<bool>,
        lazy_scope: Option<LazyS>,
        use_scope_as_lazy: bool,
    ) -> Result<Self::Output>
    where
        PreS: SymbolLookup + ?Sized,
        PostS: SymbolLookup + ?Sized,
        LazyS: SymbolLookup + Send + Sync + 'static,
        PreH: RelocationHandler,
        PostH: RelocationHandler;
}

/// A builder for configuring and executing the relocation process.
///
/// `Relocator` provides a fluent interface for setting up symbol resolution
/// strategies, relocation handlers, and binding behaviors before performing
/// the actual relocation of an ELF object.
///
/// # Examples
/// ```no_run
/// use elf_loader::{Loader, ElfBinary};
///
/// let mut loader = Loader::new();
/// let bytes = &[]; // ELF file bytes
/// let lib = loader.load_dylib(ElfBinary::new("liba.so", bytes)).unwrap();
///
/// let relocated = lib.relocator()
///     .pre_find_fn(|name| {
///         if name == "malloc" { Some(0x1234 as *const ()) } else { None }
///     })
///     .lazy(true)
///     .relocate()
///     .unwrap();
/// ```
pub struct Relocator<T, PreS, PostS, LazyS, PreH, PostH, D = ()> {
    object: T,
    scope: Vec<LoadedModule<D>>,
    pre_find: PreS,
    post_find: PostS,
    pre_handler: PreH,
    post_handler: PostH,
    lazy: Option<bool>,
    lazy_scope: Option<LazyS>,
    use_scope_as_lazy: bool,
}

impl<T: Relocatable<D>, D> Relocator<T, (), (), (), (), (), D> {
    /// Creates a new `Relocator` builder for the given object.
    pub fn new(object: T) -> Self {
        Self {
            object,
            scope: Vec::new(),
            pre_find: (),
            post_find: (),
            pre_handler: (),
            post_handler: (),
            lazy: None,
            lazy_scope: None,
            use_scope_as_lazy: false,
        }
    }
}

impl<T, PreS, PostS, LazyS, PreH, PostH, D> Relocator<T, PreS, PostS, LazyS, PreH, PostH, D>
where
    T: Relocatable<D>,
    PreS: SymbolLookup,
    PostS: SymbolLookup,
    LazyS: SymbolLookup + Send + Sync + 'static,
    PreH: RelocationHandler,
    PostH: RelocationHandler,
{
    /// Sets the preferred symbol lookup strategy.
    ///
    /// Symbols will be searched using this strategy before checking the
    /// default scope or fallback strategies.
    pub fn pre_find<S2>(self, pre_find: S2) -> Relocator<T, S2, PostS, LazyS, PreH, PostH, D>
    where
        S2: SymbolLookup,
    {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find,
            post_find: self.post_find,
            pre_handler: self.pre_handler,
            post_handler: self.post_handler,
            lazy: self.lazy,
            lazy_scope: self.lazy_scope,
            use_scope_as_lazy: self.use_scope_as_lazy,
        }
    }

    /// Sets the preferred symbol lookup strategy using a closure.
    pub fn pre_find_fn(
        self,
        pre_find: impl Fn(&str) -> Option<*const ()>,
    ) -> Relocator<T, impl Fn(&str) -> Option<*const ()>, PostS, LazyS, PreH, PostH, D> {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find,
            post_find: self.post_find,
            pre_handler: self.pre_handler,
            post_handler: self.post_handler,
            lazy: self.lazy,
            lazy_scope: self.lazy_scope,
            use_scope_as_lazy: self.use_scope_as_lazy,
        }
    }

    /// Sets the fallback symbol lookup strategy using a closure.
    ///
    /// This strategy will be used if a symbol is not found in the preferred
    /// strategy or the default scope.
    pub fn post_find_fn(
        self,
        post_find: impl Fn(&str) -> Option<*const ()>,
    ) -> Relocator<T, PreS, impl Fn(&str) -> Option<*const ()>, LazyS, PreH, PostH, D> {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find: self.pre_find,
            post_find,
            pre_handler: self.pre_handler,
            post_handler: self.post_handler,
            lazy: self.lazy,
            lazy_scope: self.lazy_scope,
            use_scope_as_lazy: self.use_scope_as_lazy,
        }
    }

    /// Sets the fallback symbol lookup strategy.
    ///
    /// This strategy will be used if a symbol is not found in the preferred
    /// strategy or the default scope.
    pub fn post_find<S2>(self, post_find: S2) -> Relocator<T, PreS, S2, LazyS, PreH, PostH, D>
    where
        S2: SymbolLookup,
    {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find: self.pre_find,
            post_find,
            pre_handler: self.pre_handler,
            post_handler: self.post_handler,
            lazy: self.lazy,
            lazy_scope: self.lazy_scope,
            use_scope_as_lazy: self.use_scope_as_lazy,
        }
    }

    /// Sets the scope of relocated libraries for symbol resolution.
    ///
    /// The relocator will search for symbols in these libraries in the order
    /// they are provided.
    pub fn scope<I, R>(mut self, scope: I) -> Self
    where
        I: IntoIterator<Item = R>,
        R: core::borrow::Borrow<LoadedModule<D>>,
    {
        self.scope = scope.into_iter().map(|r| r.borrow().clone()).collect();
        self
    }

    /// Sets the pre-processing relocation handler.
    ///
    /// This handler is called before the default relocation logic.
    pub fn pre_handler<NewPreH>(
        self,
        handler: NewPreH,
    ) -> Relocator<T, PreS, PostS, LazyS, NewPreH, PostH, D>
    where
        NewPreH: RelocationHandler,
    {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find: self.pre_find,
            post_find: self.post_find,
            pre_handler: handler,
            post_handler: self.post_handler,
            lazy: self.lazy,
            lazy_scope: self.lazy_scope,
            use_scope_as_lazy: self.use_scope_as_lazy,
        }
    }

    /// Sets the post-processing relocation handler.
    ///
    /// This handler is called after the default relocation logic if the
    /// relocation was not already handled.
    pub fn post_handler<NewPostH>(
        self,
        handler: NewPostH,
    ) -> Relocator<T, PreS, PostS, LazyS, PreH, NewPostH, D>
    where
        NewPostH: RelocationHandler,
    {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find: self.pre_find,
            post_find: self.post_find,
            pre_handler: self.pre_handler,
            post_handler: handler,
            lazy: self.lazy,
            lazy_scope: self.lazy_scope,
            use_scope_as_lazy: self.use_scope_as_lazy,
        }
    }

    /// Enables or disables lazy binding.
    ///
    /// If enabled, some relocations (typically PLT entries) will be resolved
    /// on-demand when the function is first called.
    pub fn lazy(mut self, lazy: bool) -> Self {
        self.lazy = Some(lazy);
        self
    }

    /// Sets the lazy scope for symbol resolution during lazy binding.
    pub fn lazy_scope<NewLazyS>(
        self,
        scope: NewLazyS,
    ) -> Relocator<T, PreS, PostS, NewLazyS, PreH, PostH, D>
    where
        NewLazyS: SymbolLookup + Send + Sync + 'static,
    {
        Relocator {
            object: self.object,
            scope: self.scope,
            pre_find: self.pre_find,
            post_find: self.post_find,
            pre_handler: self.pre_handler,
            post_handler: self.post_handler,
            lazy: self.lazy,
            lazy_scope: Some(scope),
            use_scope_as_lazy: self.use_scope_as_lazy,
        }
    }

    /// Uses the provided scope as the lazy scope for symbol resolution.
    pub fn use_scope_as_lazy(mut self) -> Self {
        self.use_scope_as_lazy = true;
        self
    }

    /// Executes the relocation process.
    ///
    /// This method consumes the relocator and returns the relocated ELF object.
    ///
    /// # Returns
    /// * `Ok(T::Output)` - The relocated ELF object.
    /// * `Err(Error)` - If relocation fails.
    pub fn relocate(self) -> Result<T::Output>
    where
        D: 'static,
    {
        self.object.relocate(
            &self.scope,
            &self.pre_find,
            &self.post_find,
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

pub struct SymDef<'lib, D> {
    pub sym: Option<&'lib ElfSymbol>,
    pub lib: &'lib ElfModule<D>,
}

impl<'temp, D> SymDef<'temp, D> {
    // 获取符号的真实地址(base + st_value)
    pub fn convert(self) -> *const () {
        if likely(self.sym.is_some()) {
            let base = self.lib.base();
            let sym = unsafe { self.sym.unwrap_unchecked() };
            let addr = base + sym.st_value();
            if likely(sym.st_type() != STT_GNU_IFUNC) {
                addr as _
            } else {
                // IFUNC会在运行时确定地址，这里使用的是ifunc的返回值
                let ifunc: fn() -> usize = unsafe { core::mem::transmute(addr) };
                ifunc() as _
            }
        } else {
            // 未定义的弱符号返回null
            null()
        }
    }
}

#[cold]
pub(crate) fn reloc_error<D, E: core::fmt::Display>(
    rel: &ElfRelType,
    err: E,
    lib: &ElfModule<D>,
) -> Error {
    let r_type_str = rel.r_type_str();
    let r_sym = rel.r_symbol();
    if r_sym == 0 {
        relocate_error(format!(
            "file: {}, relocation type: {}, no symbol, error: {}",
            lib.shortname(),
            r_type_str,
            err
        ))
    } else {
        relocate_error(format!(
            "file: {}, relocation type: {}, symbol name: {}, error: {}",
            lib.shortname(),
            r_type_str,
            lib.symtab().symbol_idx(r_sym).1.name(),
            err
        ))
    }
}

fn find_weak<'lib, D>(lib: &'lib ElfModule<D>, dynsym: &'lib ElfSymbol) -> Option<SymDef<'lib, D>> {
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

#[inline]
pub(crate) fn find_symbol_addr<PreS, PostS, D>(
    pre_find: &PreS,
    post_find: &PostS,
    core: &ElfModule<D>,
    symtab: &SymbolTable,
    scope: &[LoadedModule<D>],
    r_sym: usize,
) -> Option<(RelocValue<usize>, Option<usize>)>
where
    PreS: SymbolLookup + ?Sized,
    PostS: SymbolLookup + ?Sized,
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
    if let Some(res) = find_symdef_impl(core, scope, dynsym, &syminfo) {
        return Some((RelocValue::new(res.0.convert() as usize), res.1));
    }
    if let Some(addr) = post_find.lookup(syminfo.name()) {
        #[cfg(feature = "log")]
        log::trace!(
            "binding file [{}] to [post_find]: symbol [{}]",
            core.name(),
            syminfo.name()
        );
        return Some((RelocValue::new(addr as usize), None));
    }
    None
}

fn find_symdef_impl<'lib, D>(
    core: &'lib ElfModule<D>,
    scope: &'lib [LoadedModule<D>],
    sym: &'lib ElfSymbol,
    syminfo: &SymbolInfo,
) -> Option<(SymDef<'lib, D>, Option<usize>)> {
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
        scope
            .iter()
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
