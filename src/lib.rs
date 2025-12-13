//! # elf_loader
//! A `safe`, `lightweight`, `extensible`, and `high-performance` library for loading ELF files.
//!
//! ## Features
//! * Safe: Written in pure Rust with no unsafe code in the API
//! * Lightweight: Zero dependencies in the core library
//! * Extensible: Trait-based architecture allows customization for different platforms
//! * High-performance: Optimized for fast loading and symbol resolution
//! * Cross-platform: Supports multiple architectures (x86_64, aarch64, riscv, etc.)
//! * No-std compatible: Can be used in kernel and embedded environments
//!
//! ## Usage
//! `elf_loader` can load various ELF files and provides interfaces for extended functionality. It can be used in the following areas:
//! * Use it as an ELF file loader in operating system kernels
//! * Use it to implement a Rust version of the dynamic linker
//! * Use it to load ELF dynamic libraries on embedded devices
//!
//! ## Example
//! ```rust
//! use elf_loader::{Loader, object::ElfFile, Relocatable};
//! use std::collections::HashMap;
//!
//! fn print(s: &str) {
//!     println!("{}", s);
//! }
//! // Symbols required by dynamic library liba.so
//! let mut map = HashMap::new();
//! map.insert("print", print as _);
//! let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
//! // Load dynamic library liba.so
//! let mut loader = Loader::new();
//! let liba = loader
//!     .load_dylib(ElfFile::from_path("target/liba.so").unwrap())
//!     .unwrap()
//!     .relocator()
//!     .symbols(&pre_find)
//!     .relocate()
//!     .unwrap();
//! // Call function a in liba.so
//! let f = unsafe { liba.get::<fn() -> i32>("a").unwrap() };
//! f();
//! ```
#![no_std]
#![warn(
    clippy::unnecessary_wraps,
    clippy::unnecessary_lazy_evaluations,
    clippy::collapsible_if,
    clippy::cast_lossless,
    clippy::explicit_iter_loop,
    clippy::manual_assert,
    clippy::needless_question_mark,
    clippy::needless_return,
    clippy::needless_update,
    clippy::redundant_clone,
    clippy::redundant_else,
    clippy::redundant_static_lifetimes
)]
#![allow(
    clippy::len_without_is_empty,
    clippy::unnecessary_cast,
    clippy::uninit_vec
)]
extern crate alloc;

/// Compile-time check for supported architectures
#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "riscv64",
    target_arch = "riscv32",
    target_arch = "loongarch64",
    target_arch = "x86",
    target_arch = "arm",
)))]
compile_error!(
    "Unsupported target architecture. Supported architectures: x86_64, aarch64, riscv64, riscv32, loongarch64, x86, arm"
);

pub mod arch;
pub mod dynamic;
mod ehdr;
mod format;
mod hash;
mod loader;
mod macros;
pub mod mmap;
pub mod object;
mod os;
mod relocation;
pub mod segment;
mod symbol;
#[cfg(feature = "version")]
mod version;

use alloc::borrow::Cow;
use core::fmt::{Debug, Display};
use object::*;

pub use elf::abi;
pub use format::relocatable::ElfRelocatable;
pub use format::relocated::{ElfDylib, ElfExec, RelocatedDylib, RelocatedExec};
pub use format::{CoreComponent, CoreComponentRef, Elf, Relocated, Symbol, UserData};
pub use loader::{Hook, Loader};
pub use relocation::{Relocatable, RelocationHandler, SymbolLookup};

/// Error types used throughout the elf_loader library
///
/// These errors represent various failure conditions that can occur during
/// ELF file loading, parsing, and relocation operations.
#[derive(Debug)]
pub enum Error {
    /// An error occurred while opening, reading, or writing ELF files
    ///
    /// This error typically indicates issues with file I/O operations such as:
    /// * File not found
    /// * Permission denied
    /// * I/O errors during read/write operations
    Io {
        /// A descriptive message about the I/O error
        msg: Cow<'static, str>,
    },

    /// An error occurred during memory mapping operations
    ///
    /// This error typically indicates issues with memory management operations such as:
    /// * Failed to map file into memory
    /// * Failed to change memory protection
    /// * Failed to unmap memory regions
    Mmap {
        /// A descriptive message about the memory mapping error
        msg: Cow<'static, str>,
    },

    /// An error occurred during dynamic library relocation
    ///
    /// This error typically indicates issues with symbol resolution or relocation
    /// operations such as:
    /// * Undefined symbols
    /// * Incompatible symbol types
    /// * Relocation calculation errors
    Relocation {
        /// A descriptive message about the relocation error
        msg: Cow<'static, str>,
    },

    /// An error occurred while parsing the dynamic section
    ///
    /// This error typically indicates issues with parsing the .dynamic section such as:
    /// * Invalid dynamic entry types
    /// * Missing required dynamic entries
    /// * Malformed dynamic section data
    ParseDynamic {
        /// A descriptive message about the dynamic section parsing error
        msg: Cow<'static, str>,
    },

    /// An error occurred while parsing the ELF header
    ///
    /// This error typically indicates issues with the ELF header such as:
    /// * Invalid magic bytes
    /// * Unsupported ELF class or data encoding
    /// * Invalid ELF header fields
    ParseEhdr {
        /// A descriptive message about the ELF header parsing error
        msg: Cow<'static, str>,
    },

    /// An error occurred while parsing program headers
    ///
    /// This error typically indicates issues with program header parsing such as:
    /// * Invalid program header types
    /// * Malformed program header data
    /// * Incompatible program header entries
    ParsePhdr {
        /// A descriptive message about the program header parsing error
        msg: Cow<'static, str>,
    },

    /// An error occurred in a user-defined callback or handler
    Custom {
        /// A descriptive message about the custom error
        msg: Cow<'static, str>,
    },
}

impl Display for Error {
    /// Formats the error for display purposes
    ///
    /// This implementation provides human-readable error messages for all error variants.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Io { msg } => write!(f, "I/O error: {msg}"),
            Error::Mmap { msg } => write!(f, "Memory mapping error: {msg}"),
            Error::Relocation { msg, .. } => write!(f, "Relocation error: {msg}"),
            Error::ParseDynamic { msg } => write!(f, "Dynamic section parsing error: {msg}"),
            Error::ParseEhdr { msg } => write!(f, "ELF header parsing error: {msg}"),
            Error::ParsePhdr { msg, .. } => write!(f, "Program header parsing error: {msg}"),
            Error::Custom { msg } => write!(f, "Custom error: {msg}"),
        }
    }
}

impl core::error::Error for Error {}

/// Creates an I/O error with the specified message
///
/// This is a convenience function for creating IOError variants.
///
/// # Arguments
/// * `msg` - The error message
///
/// # Returns
/// An Error::Io variant with the specified message
#[cold]
#[inline(never)]
#[allow(unused)]
fn io_error(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::Io { msg: msg.into() }
}

/// Creates a relocation error with the specified message
///
/// This is a convenience function for creating RelocateError variants.
///
/// # Arguments
/// * `msg` - The error message
///
/// # Returns
/// An Error::Relocation variant with the specified message
#[cold]
#[inline(never)]
fn relocate_error(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::Relocation { msg: msg.into() }
}

/// Creates a dynamic section parsing error with the specified message
///
/// This is a convenience function for creating ParseDynamicError variants.
///
/// # Arguments
/// * `msg` - The error message
///
/// # Returns
/// An Error::ParseDynamic variant with the specified message
#[cold]
#[inline(never)]
fn parse_dynamic_error(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::ParseDynamic { msg: msg.into() }
}

/// Creates an ELF header parsing error with the specified message
///
/// This is a convenience function for creating ParseEhdrError variants.
///
/// # Arguments
/// * `msg` - The error message
///
/// # Returns
/// An Error::ParseEhdr variant with the specified message
#[cold]
#[inline(never)]
fn parse_ehdr_error(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::ParseEhdr { msg: msg.into() }
}

/// Creates a custom error with the specified message
///
/// This is a convenience function for creating Custom variants.
///
/// # Arguments
/// * `msg` - The error message
///
/// # Returns
/// An Error::Custom variant with the specified message
#[cold]
#[inline(never)]
#[allow(unused)]
pub fn custom_error(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::Custom { msg: msg.into() }
}

/// Set the global scope for symbol resolution
///
/// This function sets a global symbol resolution function that will be used

/// A type alias for Results returned by elf_loader functions
///
/// This is a convenience alias that eliminates the need to repeatedly specify
/// the Error type in function signatures.
pub type Result<T> = core::result::Result<T, Error>;
