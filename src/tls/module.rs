use super::{TlsBinding, TlsIndex, TlsModuleId, TlsTpOffset};
use crate::{
    Result,
    memory::VmAddr,
    relocation::RelocationArch,
    runtime::{CodeContext, CodeExecutor, DomainId},
};
use alloc::boxed::Box;
use core::fmt;

/// TLS registration metadata and its runtime entry address.
///
/// The owning [`TlsRegistration`] must outlive every use of this metadata.
/// Cloning or dropping a `TlsModule` does not retain or unregister it.
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
    /// Creates registration metadata for a custom runtime.
    ///
    /// # Safety
    /// `binding` must describe a live registration with valid ABI metadata for
    /// `entry`. Keep the runtime and entry fixed and alive for every use, and
    /// transfer cleanup responsibility to a [`TlsRegistration`].
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

/// Owns a TLS registration and unregisters it on drop.
///
/// Keep it alive through module finalization and TLS descriptor cleanup.
/// Cloning [`TlsModule`] does not extend the registration's lifetime.
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
