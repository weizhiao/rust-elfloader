use super::{CodeExecutor, NativeCodeExecutor};
use crate::{
    Error, Result,
    arch::NativeArch,
    memory::VmAddr,
    relocation::RelocationArch,
    sync::{Arc, AtomicUsize, Ordering, arc_unsize},
    tls::TlsResolver,
};
use alloc::boxed::Box;
use core::fmt;

static NEXT_ADDRESS_SPACE_ID: AtomicUsize = AtomicUsize::new(1);

/// Identity of a target address space and its compatible code execution environment.
///
/// Use distinct identities for independent processes, guests, or devices. Executors
/// and mapping backends used in one address space must interpret addresses alike.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AddressSpaceId(usize);

impl AddressSpaceId {
    /// The current native process's address space.
    pub const PROCESS: Self = Self(0);

    /// Creates an identity for an independent address space.
    pub fn new() -> Self {
        Self(
            #[allow(deprecated)]
            NEXT_ADDRESS_SPACE_ID
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("address space identity exhausted"),
        )
    }
}

impl Default for AddressSpaceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AddressSpaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Immutable compatibility identity: an address space and a TLS runtime namespace.
///
/// Obtain this from [`Domain::id`]. Equal identities allow modules to participate
/// in the same scope. A TLS-disabled domain is distinct from every enabled domain.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DomainId {
    address_space: AddressSpaceId,
    tls_entry: Option<VmAddr>,
}

impl DomainId {
    /// Returns the address space in which module addresses are meaningful.
    pub const fn address_space(self) -> AddressSpaceId {
        self.address_space
    }

    /// Returns the TLS runtime entry, or `None` when TLS is disabled.
    pub const fn tls_entry(self) -> Option<VmAddr> {
        self.tls_entry
    }

    #[inline]
    pub(crate) fn ensure(self, actual: Self) -> Result<()> {
        if self == actual {
            Ok(())
        } else {
            Err(Error::DomainMismatch(Box::new(
                crate::error::DomainMismatchError {
                    expected: self,
                    actual,
                },
            )))
        }
    }
}

impl fmt::Display for DomainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.tls_entry {
            Some(entry) => write!(f, "{}:tls@{:#x}", self.address_space, entry.get()),
            None => write!(f, "{}:no-tls", self.address_space),
        }
    }
}

/// An address space, executor, and optional TLS resolver used to load and run code.
///
/// Clone a domain to reuse its configuration. Domains with the same address space
/// and TLS entry have equal [`DomainId`] values and can share modules.
pub struct Domain<Arch: RelocationArch = NativeArch, Tls: TlsResolver<Arch> = ()> {
    id: DomainId,
    tls: Tls,
    executor: Arc<dyn CodeExecutor<Arch>>,
}

impl<Arch: RelocationArch, Tls: TlsResolver<Arch>> Domain<Arch, Tls> {
    /// Returns the shared executor used by every module in this domain.
    pub fn executor(&self) -> &dyn CodeExecutor<Arch> {
        self.executor.as_ref()
    }

    pub(crate) fn shared_executor(&self) -> Arc<dyn CodeExecutor<Arch>> {
        self.executor.clone()
    }

    /// Returns the immutable identity used for module compatibility checks.
    pub const fn id(&self) -> DomainId {
        self.id
    }

    /// Returns the fixed TLS resolver used for registration and runtime binding.
    pub const fn tls_resolver(&self) -> &Tls {
        &self.tls
    }
}

impl<Arch: RelocationArch> Domain<Arch, ()> {
    /// Creates a domain with TLS explicitly disabled, without querying an entry.
    pub fn new<E: CodeExecutor<Arch>>(address_space: AddressSpaceId, executor: E) -> Self {
        Self {
            id: DomainId {
                address_space,
                tls_entry: None,
            },
            tls: (),
            executor: arc_unsize!(Arc::new(executor) => dyn CodeExecutor<Arch>),
        }
    }

    /// Enables TLS, returning a domain with the resolver's runtime identity.
    ///
    /// The resolver must target this address space and agree with the executor.
    /// Entry lookup errors are returned; existing domain clones remain unchanged.
    pub fn with_tls<Tls: TlsResolver<Arch>>(self, tls: Tls) -> Result<Domain<Arch, Tls>> {
        let entry = tls.bind_tls_get_addr()?;
        Ok(Domain {
            id: DomainId {
                address_space: self.id.address_space,
                tls_entry: Some(entry),
            },
            tls,
            executor: self.executor,
        })
    }
}

impl Domain<NativeArch, ()> {
    /// Creates a TLS-disabled domain executing code in the current process.
    pub fn native(address_space: AddressSpaceId) -> Self {
        Self::new(address_space, NativeCodeExecutor)
    }
}

impl<Arch: RelocationArch, Tls: TlsResolver<Arch>> Clone for Domain<Arch, Tls> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            tls: self.tls.clone(),
            executor: self.executor.clone(),
        }
    }
}

impl<Arch: RelocationArch, Tls: TlsResolver<Arch>> fmt::Debug for Domain<Arch, Tls> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Domain")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
