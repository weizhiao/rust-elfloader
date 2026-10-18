use crate::{
    arch::NativeArch,
    image::{LocalScope, RawDynamic},
    memory::{HostRegion, RegionAccess},
    relocation::RelocationArch,
    tls::TlsResolver,
};

/// Event exposing the image and lookup scope for one module's relocation.
pub struct LinkerRelocationEvent<
    D: Send + Sync + 'static,
    Arch: RelocationArch = NativeArch,
    R: RegionAccess = HostRegion,
    Tls: TlsResolver<Arch> = (),
> {
    raw: RawDynamic<D, Arch, R, Tls>,
    scope: LocalScope<Arch>,
}

impl<D: Send + Sync + 'static, Arch, R, Tls> LinkerRelocationEvent<D, Arch, R, Tls>
where
    Arch: RelocationArch,
    R: RegionAccess,
    Tls: TlsResolver<Arch>,
{
    #[inline]
    pub(crate) fn new(raw: RawDynamic<D, Arch, R, Tls>, scope: LocalScope<Arch>) -> Self {
        Self { raw, scope }
    }

    /// Returns the loaded image that is about to be relocated.
    #[inline]
    pub const fn raw(&self) -> &RawDynamic<D, Arch, R, Tls> {
        &self.raw
    }

    /// Returns the module's lookup scope for this relocation.
    ///
    /// Linker-global modules participate in relocation separately and are not
    /// retained by this scope.
    #[inline]
    pub const fn scope(&self) -> &LocalScope<Arch> {
        &self.scope
    }

    /// Returns the mutable lookup scope used to relocate this module.
    /// Scope mutation admits only compatible groups and reports errors before
    /// changing the scope. Replacing the entire scope is checked when attached
    /// to the relocation run.
    ///
    /// Linker-global modules are managed separately and are not part of this
    /// scope.
    #[inline]
    pub const fn scope_mut(&mut self) -> &mut LocalScope<Arch> {
        &mut self.scope
    }

    #[inline]
    pub(crate) fn into_parts(self) -> (RawDynamic<D, Arch, R, Tls>, LocalScope<Arch>) {
        (self.raw, self.scope)
    }
}
