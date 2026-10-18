use super::{TlsBinding, TlsIndex, TlsModuleId, TlsTpOffset};
use crate::{
    Result,
    memory::VmAddr,
    relocation::RelocationArch,
    runtime::{CodeContext, CodeExecutor, DomainId},
};
use alloc::boxed::Box;
use core::fmt;

/// Non-owning metadata for a TLS registration and its runtime ABI entry.
///
/// Cloning or dropping this value does not retain or unregister the module.
/// A [`TlsRegistration`] owns the registry entry and runtime resources. Its
/// lifetime must cover every use of this metadata. Imported providers retain
/// their original storage; managed providers retain immutable template bytes.
/// The containing image and its executor determine the target architecture;
/// this metadata does not encode it.
#[derive(Clone)]
pub struct TlsModule {
    binding: TlsBinding,
    entry: VmAddr,
}

impl fmt::Debug for TlsModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TlsModule").field(&self.binding).finish()
    }
}

impl TlsModule {
    /// Creates metadata for a newly registered module. Intended for runtime implementations.
    ///
    /// # Safety
    /// `binding` must describe a live registration in the runtime identified by
    /// `entry`, with correct ABI metadata. The runtime and ABI entry must remain
    /// fixed and alive while this metadata is in use.
    /// The runtime must transfer its cleanup responsibility into a
    /// [`TlsRegistration`]. Constructing or cloning this metadata does not
    /// transfer or share that responsibility.
    pub const unsafe fn new(binding: TlsBinding, entry: VmAddr) -> Self {
        Self { binding, entry }
    }

    /// Returns the relocation metadata without transferring ownership.
    pub const fn binding(&self) -> TlsBinding {
        self.binding
    }
    /// Returns the runtime module ID.
    pub const fn mod_id(&self) -> TlsModuleId {
        self.binding.mod_id()
    }
    /// Returns a stable thread-pointer offset, when available.
    pub const fn tp_offset(&self) -> Option<TlsTpOffset> {
        self.binding.tp_offset()
    }

    pub(crate) fn check_domain(&self, domain: DomainId) -> Result<()> {
        if Some(self.entry) != domain.tls_entry() {
            return Err(crate::TlsError::ModuleMismatch.into());
        }
        Ok(())
    }

    /// Returns this registration's runtime ABI entry.
    pub const fn entry(&self) -> VmAddr {
        self.entry
    }

    pub(crate) fn resolve<Arch: RelocationArch>(
        &self,
        executor: &dyn CodeExecutor<Arch>,
        ctx: CodeContext<'_, Arch>,
        offset: usize,
    ) -> Result<VmAddr> {
        executor.resolve_tls(
            ctx,
            self.entry,
            TlsIndex {
                ti_module: self.mod_id(),
                ti_offset: offset.wrapping_sub(Arch::TLS_DTV_OFFSET),
            },
        )
    }
}

/// Unique owner of a TLS registry entry.
///
/// Loaded and synthetic images retain this value until their finalizers and TLS
/// descriptor resources have been released. The cleanup closure also keeps the
/// resolver alive. Cloning [`TlsModule`] does not extend this ownership.
pub struct TlsRegistration {
    module: TlsModule,
    release: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl TlsRegistration {
    /// Takes ownership of a registration created by a runtime implementation.
    ///
    /// # Safety
    /// `module` must describe a live registration. `release` must take sole
    /// ownership of its cleanup, retain the runtime, and remove that registration
    /// exactly once. It may own a previous guard to compose additional cleanup.
    /// Provider destruction and callbacks must run without registry locks held.
    pub unsafe fn new(module: TlsModule, release: impl FnOnce() + Send + Sync + 'static) -> Self {
        Self {
            module,
            release: Some(Box::new(release)),
        }
    }

    /// Borrows ABI metadata without transferring registration ownership.
    pub fn module(&self) -> &TlsModule {
        &self.module
    }
}

impl fmt::Debug for TlsRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TlsRegistration")
            .field(&self.module)
            .finish()
    }
}

impl Drop for TlsRegistration {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release();
        }
    }
}
