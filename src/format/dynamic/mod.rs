//! Relocated ELF file handling
//!
//! This module provides functionality for working with ELF files that have
//! been loaded and relocated in memory. It includes support for both dynamic
//! libraries (shared objects) and executables.

pub(crate) use common::{DynamicBuilder, DynamicComponent, DynamicInfo};
pub use dylib::{DylibImage, LoadedDylib};
pub use exec::{ExecImage, LoadedExec};

mod common;
pub(crate) mod dylib;
pub(crate) mod exec;
