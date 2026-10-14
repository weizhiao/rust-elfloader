//! Lazy PLT binding support.
//!
//! The [`LazyBinder`] trait lets callers install target-specific lazy binding
//! entries. With the `lazy-binding` feature enabled, `NativeLazyBinder` provides
//! the same-process native binder used for ordinary host execution.

mod defs;
#[cfg(feature = "lazy-binding")]
mod native;
mod relocation;
mod runtime;
mod traits;

pub use defs::{LazyPlacement, LazySetup, LazyValues};
#[cfg(feature = "lazy-binding")]
pub use native::NativeLazyBinder;
#[cfg(feature = "lazy-binding")]
pub(crate) use native::dl_fixup;
pub(crate) use relocation::{install_plt, relocate_jump_slot};
pub use runtime::{LazyPltReloc, LazyRuntime};
pub use traits::{LazyBinder, SupportLazy};
