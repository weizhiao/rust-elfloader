use super::storage::ModuleId;
use crate::{
    arch::NativeArch,
    image::{Module, ModuleHandle, ModuleScope},
    relocation::RelocationArch,
};
use alloc::vec::Vec;
use core::{fmt, ops::Deref};

/// One module detached from a link context by [`super::LinkContext::release`].
pub struct UnloadedModule<Meta = (), Arch: RelocationArch = NativeArch> {
    id: ModuleId,
    module: ModuleHandle<Arch>,
    _retained: ModuleScope<Arch>,
    meta: Meta,
}

impl<Meta, Arch> UnloadedModule<Meta, Arch>
where
    Arch: RelocationArch,
{
    #[inline]
    pub(super) const fn new(
        id: ModuleId,
        module: ModuleHandle<Arch>,
        retained: ModuleScope<Arch>,
        meta: Meta,
    ) -> Self {
        Self {
            id,
            module,
            _retained: retained,
            meta,
        }
    }

    /// Returns the id the module had before it was detached.
    #[inline]
    pub const fn id(&self) -> ModuleId {
        self.id
    }

    /// Borrows the detached module while its unload group retains dependencies.
    #[inline]
    pub fn module(&self) -> &dyn Module<Arch> {
        self.module.as_ref()
    }

    /// Returns metadata detached with this context entry.
    #[inline]
    pub const fn meta(&self) -> &Meta {
        &self.meta
    }

    /// Consumes the entry and returns its context metadata.
    ///
    /// The module and retained lookup scope are dropped together so a bare
    /// module handle cannot outlive its relocation providers accidentally.
    #[inline]
    pub fn into_meta(self) -> Meta {
        self.meta
    }
}

impl<Meta, Arch> fmt::Debug for UnloadedModule<Meta, Arch>
where
    Arch: RelocationArch,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnloadedModule")
            .field("id", &self.id)
            .field("name", &self.module().name())
            .finish_non_exhaustive()
    }
}

/// Modules made unreachable and detached by [`super::LinkContext::release`].
///
/// The collection retains the complete unload group so finalizers may still
/// call code in dependencies that were detached at the same time. It dereferences
/// to the detached modules in finalization order.
pub struct UnloadGroup<Meta = (), Arch: RelocationArch = NativeArch> {
    modules: Vec<UnloadedModule<Meta, Arch>>,
}

impl<Meta, Arch> UnloadGroup<Meta, Arch>
where
    Arch: RelocationArch,
{
    #[inline]
    pub(super) fn new(modules: Vec<UnloadedModule<Meta, Arch>>) -> Self {
        Self { modules }
    }
}

impl<Meta, Arch> Deref for UnloadGroup<Meta, Arch>
where
    Arch: RelocationArch,
{
    type Target = [UnloadedModule<Meta, Arch>];

    /// Returns detached modules in finalization order.
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.modules
    }
}

impl<Meta, Arch> fmt::Debug for UnloadGroup<Meta, Arch>
where
    Arch: RelocationArch,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.modules).finish()
    }
}
