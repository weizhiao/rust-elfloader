use crate::{
    Relocator, Result,
    arch::NativeArch,
    image::{GlobalScope, LocalScope, ModuleHandle, ModuleScope},
    lazy::{LazyBinder, SupportLazy},
    observer::RelocationObserver,
    relocation::{
        BindingMode, LookupOrder, Relocatable, RelocateArgs, RelocationArch, SymbolRegistry,
    },
    sync::Arc,
};

/// Per-image relocation run state.
///
/// A run owns the raw image being relocated and the scope being assembled for
/// that image. It consumes itself when [`relocate`](Self::relocate) is called.
pub struct RelocatorRun<'cfg, T, Arch: RelocationArch = NativeArch, Obs = (), Binder = ()> {
    object: T,
    scope: LocalScope<Arch>,
    global: Option<GlobalScope<Arch>>,
    observer: Obs,
    binding: BindingMode,
    lookup_order: LookupOrder,
    symbols: Option<Arc<SymbolRegistry<Arch>>>,
    relocator: &'cfg Relocator<Binder>,
}

impl<Binder> Relocator<Binder> {
    /// Starts a relocation run using this reusable configuration.
    #[inline]
    pub fn run<D: Send + Sync + 'static, T, Arch>(
        &self,
        object: T,
    ) -> RelocatorRun<'_, T, Arch, (), Binder>
    where
        Arch: RelocationArch,
        T: Relocatable<D, Arch = Arch>,
    {
        let domain = object.domain().id();
        self.start_run(object, LocalScope::empty(domain))
    }

    /// Starts a run with its final scope, avoiding an unused empty allocation.
    pub(crate) fn run_with_local_scope<D: Send + Sync + 'static, T, Arch>(
        &self,
        object: T,
        scope: LocalScope<Arch>,
    ) -> Result<RelocatorRun<'_, T, Arch, (), Binder>>
    where
        Arch: RelocationArch,
        T: Relocatable<D, Arch = Arch>,
    {
        object.domain().id().ensure(scope.domain_id())?;
        Ok(self.start_run(object, scope))
    }

    #[inline]
    fn start_run<T, Arch: RelocationArch>(
        &self,
        object: T,
        scope: LocalScope<Arch>,
    ) -> RelocatorRun<'_, T, Arch, (), Binder> {
        RelocatorRun {
            object,
            scope,
            global: None,
            observer: (),
            binding: self.binding,
            lookup_order: LookupOrder::GlobalFirst,
            symbols: None,
            relocator: self,
        }
    }
}

impl<'cfg, T, Arch, Obs, Binder> Clone for RelocatorRun<'cfg, T, Arch, Obs, Binder>
where
    Arch: RelocationArch,
    T: Clone,
    Obs: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            scope: self.scope.clone(),
            global: self.global.clone(),
            observer: self.observer.clone(),
            binding: self.binding,
            lookup_order: self.lookup_order,
            symbols: self.symbols.clone(),
            relocator: self.relocator,
        }
    }
}

impl<'cfg, T, Arch, Obs, Binder> RelocatorRun<'cfg, T, Arch, Obs, Binder>
where
    Arch: RelocationArch,
    Binder: LazyBinder<Arch>,
{
    /// Sets the runtime-linker observer used during relocation.
    pub fn with_observer<NewObs>(
        self,
        observer: NewObs,
    ) -> RelocatorRun<'cfg, T, Arch, NewObs, Binder>
    where
        NewObs: RelocationObserver<Arch>,
    {
        let RelocatorRun {
            object,
            scope,
            global,
            binding,
            lookup_order,
            symbols,
            relocator,
            observer: _,
        } = self;

        RelocatorRun {
            object,
            scope,
            global,
            observer,
            binding,
            lookup_order,
            symbols,
            relocator,
        }
    }

    /// Overrides the relocation binding mode.
    #[inline]
    pub fn with_binding(mut self, binding: BindingMode) -> Self {
        self.binding = binding;
        self
    }

    /// Sets precedence between the local and context-global lookup scopes.
    #[inline]
    pub fn with_lookup_order(mut self, order: LookupOrder) -> Self {
        self.lookup_order = order;
        self
    }

    #[inline]
    pub(crate) fn symbol_registry(mut self, symbols: Arc<SymbolRegistry<Arch>>) -> Self {
        self.symbols = Some(symbols);
        self
    }

    /// Attaches a compatible [`LinkContext`](crate::LinkContext) global scope.
    ///
    /// The shared handle is retained only for the relocation run. Deferred
    /// binding stores a weak reference and therefore does not keep the context
    /// or unrelated global modules alive.
    #[inline]
    pub fn with_global_scope(mut self, global: &GlobalScope<Arch>) -> Result<Self> {
        global.check_scope(&self.scope)?;
        self.global = Some(global.clone());
        Ok(self)
    }

    /// Admits modules for both eager and deferred lookup.
    /// Incompatible address domains or TLS runtimes are rejected before relocation.
    pub fn with_modules<I, R>(mut self, modules: I) -> Result<Self>
    where
        I: IntoIterator<Item = R>,
        R: Into<ModuleHandle<Arch>>,
    {
        let mut group = ModuleScope::new(self.scope.domain_id());
        group.replace(modules)?;
        self.scope = LocalScope::from_group(group);
        Ok(self)
    }

    /// Attaches a compatible eager and deferred lookup scope.
    pub fn with_local_scope(mut self, scope: LocalScope<Arch>) -> Result<Self> {
        self.scope.check_scope(&scope)?;
        self.scope = scope;
        Ok(self)
    }
}

impl<'cfg, T, Arch, Obs, Binder> RelocatorRun<'cfg, T, Arch, Obs, Binder>
where
    T: SupportLazy,
    Arch: RelocationArch,
    Binder: LazyBinder<Arch>,
{
    /// Forces eager binding.
    #[inline]
    pub fn eager(mut self) -> Self {
        self.binding = BindingMode::Eager;
        self
    }

    /// Forces lazy binding.
    #[inline]
    pub fn lazy(mut self) -> Self {
        self.binding = BindingMode::Lazy;
        self
    }
}

impl<'cfg, T, Arch, Obs, Binder> RelocatorRun<'cfg, T, Arch, Obs, Binder>
where
    Arch: RelocationArch,
    Obs: RelocationObserver<Arch>,
    Binder: LazyBinder<Arch>,
{
    /// Executes relocation with the current run state.
    pub fn relocate<D>(self) -> Result<<T as Relocatable<D>>::Output>
    where
        D: Send + Sync + 'static,
        T: Relocatable<D, Arch = Arch>,
    {
        let RelocatorRun {
            object,
            scope,
            global,
            mut observer,
            binding,
            lookup_order,
            symbols,
            relocator,
        } = self;

        object.relocate(RelocateArgs {
            scope,
            global,
            symbols,
            binding,
            lookup_order,
            run_init: relocator.run_init,
            lazy_binder: &relocator.lazy_binder,
            observer: &mut observer,
        })
    }
}
