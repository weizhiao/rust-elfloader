//! TLS registration, storage and runtime bindings.
//!
//! [`TlsResolver::register`] returns non-owning [`TlsModule`] metadata. The loaded
//! image owns the registration, publishes its relocated template, and unregisters
//! on drop. Imported images borrow their registrations under the caller's lifetime
//! guarantees. [`TlsBinding`] contains only ABI metadata: the module ID and an
//! optional stable thread-pointer offset.
//!
//! [`TlsProvider`] supplies storage through one interface for template publication,
//! address resolution, and non-allocating queries. [`DefaultTlsResolver`] supplies
//! a managed provider and accepts custom providers for existing host storage.
//! A host provider retains its original module and translates logical offsets
//! into its TLS ABI; its storage is not copied into a managed block.
//! [`TlsRegistry`] binds providers to immutable [`TlsContext`] metadata, while
//! [`TlsOwner`] retains provider resources independently of registry membership.
//! Callbacks and destruction run outside registry locks. [`ThreadDtv`] caches
//! only allocated dynamic blocks.
//!
//! TLS IDs are shared by each runtime entry's namespace and are never reused by
//! the default registry. Native TLSDESC bindings use the same entry and retain
//! their arguments in the image that contains the descriptor. Custom runtimes can
//! replace the resolver and override descriptor bindings for other target ABIs.
//!
//! Current-thread queries return raw pointers, not static Rust references. Their
//! validity depends on the thread and module lifetimes. Use
//! [`DefaultTlsResolver::get_ptr_if_allocated`] for non-allocating inspection.

mod defs;
mod manager;
mod module;
mod registry;
mod relocation;
mod state;
mod thread;
mod traits;

pub(crate) use defs::{TLS_GET_ADDR_SYMBOL, TlsImageProvider};
pub(crate) use state::ImageTlsState;

pub use defs::{
    TlsBinding, TlsDescBinding, TlsDescRequest, TlsDescSetup, TlsImageSource, TlsIndex, TlsInfo,
    TlsModuleId, TlsRequest, TlsTpOffset,
};
pub use manager::DefaultTlsResolver;
pub use module::TlsModule;
pub use registry::{TlsContext, TlsOwner, TlsRegistry, TlsRegistryEntry, TlsRegistrySnapshot};
pub(crate) use relocation::TlsRelocOutcome;
pub use thread::ThreadDtv;
pub use traits::{TlsProvider, TlsResolver};
