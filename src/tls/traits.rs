use super::{
    TlsContext, TlsDescBinding, TlsDescRequest, TlsDescSetup, TlsIndex, TlsInfo, TlsModuleId,
    TlsRegistration, TlsRequest, TlsTemplate,
};
use crate::{
    Result, TlsError,
    arch::{tlsdesc_resolver_dynamic, tlsdesc_resolver_static, tlsdesc_resolver_undefweak},
    memory::VmAddr,
    relocation::RelocationArch,
};
use alloc::boxed::Box;
use core::ptr::NonNull;

/// Current-process storage for a TLS registration.
///
/// The registry assigns metadata and IDs; providers own storage and decide how
/// to publish a template, resolve an address, and query an existing block.
/// Callbacks and destruction may reenter the loader and run without registry
/// locks or mutable thread-state borrows held by the caller.
///
/// # Safety
/// Returned pointers must address the registered block in the calling thread.
/// A provider must retain the runtime resources needed by its operations and
/// keep addresses valid until module or thread teardown. Implementations must
/// support concurrent calls from different threads. Host storage must retain its
/// original module and must not be freed or reinitialized by the importing runtime.
/// Managed storage must not be exposed before its relocated template is published.
/// Providers must not retain the image that owns their registration, which would
/// create an ownership cycle and prevent removal.
pub unsafe trait TlsProvider: Send + Sync + 'static {
    /// Publishes a relocated initialization template. Providers for existing
    /// storage normally retain the default rejection of publication.
    /// Call at most once per registration; repeated publication may panic.
    ///
    /// # Safety
    /// `context` must be the metadata assigned when this provider was registered.
    /// The entry may have since been removed; retained provider resources must
    /// remain usable, or the operation must report unavailable storage.
    unsafe fn publish(&self, _context: &TlsContext, _source: TlsTemplate) -> Result<()> {
        Err(TlsError::ModuleMismatch.into())
    }

    /// Resolves a logical byte offset, allocating current-thread storage if needed.
    /// The implementation handles any ABI-specific DTV bias.
    ///
    /// # Safety
    /// `context` must be the metadata assigned when this provider was registered.
    /// The entry may have since been removed; retained provider resources must
    /// remain usable, or the operation must report unavailable storage.
    unsafe fn resolve(&self, context: &TlsContext, offset: usize) -> Result<NonNull<u8>>;

    /// Queries an existing block without allocating TLS or creating thread state.
    /// `None` also means that a non-allocating query is unsupported.
    ///
    /// # Safety
    /// `context` must be the metadata assigned when this provider was registered.
    /// The entry may have since been removed; retained provider resources must
    /// remain usable, or the operation must report unavailable storage.
    unsafe fn get(&self, _context: &TlsContext) -> Option<NonNull<u8>> {
        None
    }
}

/// A trait for resolving TLS (Thread Local Storage) information.
///
/// Implement this trait to provide custom TLS module IDs and thread pointer offsets.
/// This is essential for supporting TLS in environments with custom thread management,
/// such as operating system kernels or bare-metal systems.
///
/// A [`Domain`](crate::Domain) binds a fixed resolver to an address space.
/// Link contexts and standalone [`Loader::run`](crate::Loader::run) calls share
/// that domain; its identity is used when admitting modules.
/// Every loaded module retains that handle for registration, relocation,
/// address lookup, and cleanup. Implementations may therefore keep their TLS
/// registry and runtime bindings directly in the resolver.
/// Cloning a resolver must preserve its runtime and module-ID namespace. An
/// existing handle must never switch to another backend when global configuration
/// changes; backend selection belongs in handle construction.
pub trait TlsResolver<Arch: RelocationArch>: Clone + Send + Sync + 'static {
    /// Whether this resolver should override `__tls_get_addr` symbol bindings.
    const OVERRIDE_TLS_GET_ADDR: bool = false;

    /// Registers a TLS module using the requested placement.
    ///
    /// The returned registration must belong to this resolver's runtime and
    /// module-ID namespace, with the entry returned by [`Self::bind_tls_get_addr`].
    /// The returned owner unregisters automatically after its last use. The loader
    /// transfers it into the loaded image. Cloning [`TlsModule`](super::TlsModule)
    /// only copies metadata.
    /// An error must leave no newly registered module behind.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested placement cannot be satisfied.
    fn register(&self, info: TlsInfo, request: TlsRequest) -> Result<TlsRegistration>;

    /// Publishes the final relocated template for a registered TLS module.
    ///
    /// TLS layout may be assigned before dynamic relocations have been applied.
    /// This hook is called once the template bytes are ready for future TLS block
    /// initialization. The resolver decides when and where per-thread storage is
    /// instantiated. Layout is read from the registration identified by `mod_id`.
    /// The ID must belong to this resolver's runtime and module-ID namespace,
    /// and the caller must keep the registration alive throughout publication.
    /// Call at most once per registration. Repeated publication is a lifecycle
    /// contract violation and may panic.
    fn publish(&self, source: TlsTemplate, mod_id: TlsModuleId) -> Result<()>;

    /// Removes a registration on behalf of its [`TlsRegistration`] cleanup.
    /// Runtime implementations use this operation from the owner's release
    /// closure. Users drop the owner instead of calling this method directly.
    fn unregister(&self, mod_id: TlsModuleId);

    /// Returns the target-visible `__tls_get_addr` entry point.
    ///
    /// Resolvers sharing this entry must share its module-ID namespace. IDs must
    /// not be reused while code or descriptors can still reference them.
    /// The entry and its namespace must remain unchanged for the lifetime of the
    /// resolver and every clone. [`TlsModule`](super::TlsModule) captures the entry at registration;
    /// the registration owner must keep the runtime alive for all uses.
    ///
    /// Native same-process resolvers can return a host function pointer. Remote
    /// or guest runtimes should return an address inside the target runtime.
    fn bind_tls_get_addr(&self) -> Result<VmAddr>;

    /// Returns a target-visible TLSDESC binding and state owned by its image.
    /// The default uses native ABI stubs and the entry captured in the request's
    /// registration. Non-native executors must override this operation.
    ///
    /// For a defined symbol, the caller must ensure that its registration belongs
    /// to this resolver's runtime and module-ID namespace. The loader checks this
    /// when admitting modules to a scope or importing an existing image; the
    /// default implementation does not repeat that check for each descriptor.
    /// The defining module and runtime must remain valid while the descriptor is
    /// in use.
    #[inline]
    fn bind_tlsdesc(&self, request: TlsDescRequest) -> Result<TlsDescSetup> {
        // The containing image owns these arguments. Module dependencies keep the
        // defining image alive; imported providers retain their original storage.
        #[repr(C)]
        struct DynamicArg {
            entry: usize,
            index: TlsIndex,
        }

        if !Arch::SUPPORTS_NATIVE_RUNTIME || Arch::TLSDESC.is_none() {
            return Err(TlsError::ResolverUnsupported.into());
        }
        match request {
            TlsDescRequest::Defined { module, offset } => {
                if let Some(tp_offset) = module.tp_offset() {
                    return Ok(TlsDescSetup::new(TlsDescBinding::new(
                        VmAddr::from_ptr(tlsdesc_resolver_static as *const ()),
                        VmAddr::new(offset)
                            .wrapping_add_signed(tp_offset.get())
                            .get(),
                    )));
                }
                let arg = Box::new(DynamicArg {
                    entry: module.entry().get(),
                    index: TlsIndex {
                        ti_module: module.mod_id(),
                        ti_offset: offset.wrapping_sub(Arch::TLS_DTV_OFFSET),
                    },
                });
                let ptr = VmAddr::from_ptr(arg.as_ref());
                Ok(TlsDescSetup::new(TlsDescBinding::new(
                    VmAddr::from_ptr(tlsdesc_resolver_dynamic as *const ()),
                    ptr.get(),
                ))
                .with_state(arg))
            }
            TlsDescRequest::UndefinedWeak { addend } => Ok(TlsDescSetup::new(TlsDescBinding::new(
                VmAddr::from_ptr(tlsdesc_resolver_undefweak as *const ()),
                addend,
            ))),
        }
    }
}

impl<Arch: RelocationArch> TlsResolver<Arch> for () {
    fn register(&self, _info: TlsInfo, _request: TlsRequest) -> Result<TlsRegistration> {
        Err(TlsError::ResolverUnsupported.into())
    }

    fn publish(&self, _source: TlsTemplate, _mod_id: TlsModuleId) -> Result<()> {
        Err(TlsError::ResolverUnsupported.into())
    }

    fn unregister(&self, _mod_id: TlsModuleId) {
        // No-op for unit resolver as it doesn't maintain any state
    }

    fn bind_tls_get_addr(&self) -> Result<VmAddr> {
        Err(TlsError::ResolverUnsupported.into())
    }
}
