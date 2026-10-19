//! TLS registration and per-thread storage.
//!
//! Use [`DefaultTlsResolver`] for native execution, or implement [`TlsResolver`]
//! for a custom runtime. [`TlsProvider`] supplies storage, including existing host TLS.
//!
//! [`TlsRegistration`] owns a registry entry and unregisters it on drop.
//! [`TlsModule`] is borrowed metadata; [`TlsTemplate`] owns initialization bytes
//! independently of the ELF image.
//!
//! [`DefaultTlsResolver::get_ptr_if_allocated`] queries storage without allocating.
//! Returned pointers are valid only for the corresponding thread and module lifetime.

mod defs;
mod manager;
mod module;
mod registry;
mod relocation;
mod state;
mod thread;
mod traits;

pub(crate) use defs::TLS_GET_ADDR_SYMBOL;
pub(crate) use state::ImageTlsState;

pub use defs::{
    TlsBinding, TlsDescBinding, TlsDescRequest, TlsDescSetup, TlsIndex, TlsInfo, TlsModuleId,
    TlsRequest, TlsTemplate, TlsTpOffset,
};
pub use manager::DefaultTlsResolver;
pub use module::{TlsModule, TlsRegistration};
pub use registry::{TlsContext, TlsOwner, TlsRegistry, TlsRegistryEntry, TlsRegistrySnapshot};
pub(crate) use relocation::TlsRelocOutcome;
pub use thread::ThreadDtv;
pub use traits::{TlsProvider, TlsResolver};
