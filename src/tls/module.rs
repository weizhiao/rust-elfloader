use super::{TlsBinding, TlsIndex, TlsModuleId, TlsTpOffset};
use crate::{
    Result,
    memory::VmAddr,
    relocation::RelocationArch,
    runtime::{CodeContext, CodeExecutor, DomainId},
};
use core::fmt;

/// Non-owning metadata for a TLS registration and its runtime ABI entry.
///
/// Cloning or dropping this value does not retain or unregister the module.
/// The loaded image owns registrations it creates; imported registrations must
/// remain valid under the caller's lifetime guarantees. Mapped code, templates,
/// and the runtime must also remain alive for every use of this metadata.
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
    /// The caller owns the registration and must unregister it exactly once when
    /// no longer needed. Constructing or cloning this metadata does not transfer
    /// or share that responsibility.
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
