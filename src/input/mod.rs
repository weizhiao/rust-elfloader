//! ELF object representations and access traits
//!
//! This module provides traits and implementations for accessing ELF objects,
//! whether they are stored in memory or in files. It abstracts the data source
//! to allow uniform handling of different ELF object types during the loading
//! and relocation process.

pub use backend::{ElfBinary, ElfFile};
pub use traits::{ElfReader, IntoElfReader};

mod backend;
mod traits;
