//! Runtime execution abstractions for mapped images.

mod defs;
mod native;
mod traits;

pub use defs::{AddressSpaceId, Domain, DomainId};
pub use native::NativeCodeExecutor;
pub use traits::{CodeContext, CodeExecutor};
