//! # Relink (elf_loader)
//!
//! **Relink** is a high-performance runtime linker (JIT Linker) tailor-made for the Rust ecosystem. It efficiently parses various ELF formats—not only from traditional file systems but also directly from memory images—and performs flexible dynamic and static hybrid linking.
//!
//! Whether you are developing **OS kernels**, **embedded systems**, **JIT compilers**, or building **plugin-based applications**, Relink provides a solid foundation with zero-cost abstractions, high-speed execution, and powerful extensibility.
//!
//! ## 🔥 Key Features
//!
//! ### 🛡️ Memory Safety
//! Leveraging Rust's ownership system and smart pointers, Relink ensures safety at runtime.
//! * **Lifetime Binding**: Symbols retrieved from a library carry lifetime markers. The compiler ensures they do not outlive the library itself, erasing `use-after-free` risks.
//! * **Automatic Dependency Management**: Uses `Arc` to automatically maintain dependency trees between libraries, preventing a required library from being dropped prematurely.
//!
//! ### 🔀 Hybrid Linking Capability
//! Relink supports mixing **Relocatable Object files (`.o`)** and **Dynamic Shared Objects (`.so`)**. You can load a `.o` file just like a dynamic library and link its undefined symbols to the system or other loaded libraries at runtime.
//!
//! ### 🎭 Deep Customization & Interception
//! By implementing the `SymbolLookup` and `RelocationHandler` traits, users can deeply intervene in the linking process.
//! * **Symbol Interception**: Intercept and replace external dependency symbols during loading. Perfect for function mocking, behavioral monitoring, or hot-patching.
//! * **Custom Linking Logic**: Take full control over symbol resolution strategies to build highly flexible plugin systems.
//!
//! ### ⚡ Extreme Performance & Versatility
//! * **Zero-Cost Abstractions**: Built with Rust to provide near-native loading and symbol resolution speeds.
//! * **`no_std` Support**: The core library has no OS dependencies, making it ideal for **OS kernels**, **embedded devices**, and **bare-metal development**.
//! * **Modern Features**: Supports **RELR** for modern ELF optimization; supports **Lazy Binding** to improve cold-start times for large dynamic libraries.
//!
//! ## 🚀 Quick Start
//!
//! ### Basic Example: Load and Call a Dynamic Library
//!
//! ```rust,no_run
//! use elf_loader::load_dylib;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 1. Load the library and perform instant linking
//!     let lib = load_dylib!("path/to/your_library.so")?
//!         .relocator()
//!         // Optional: Provide custom symbol resolution (e.g., export symbols from host)
//!         .pre_find_fn(|sym_name| {
//!             if sym_name == "my_host_function" {
//!                 Some(my_host_function as *const ())
//!             } else {
//!                 None
//!             }
//!         })
//!         .relocate()?; // Complete all relocations
//!
//!     // 2. Safely retrieve and call the function
//!     let awesome_func = unsafe {
//!         lib.get::<fn(i32) -> i32>("awesome_func").ok_or("symbol not found")?
//!     };
//!     let result = awesome_func(42);
//!     
//!     Ok(())
//! }
//!
//! // A host function that can be called by the plugin
//! extern "C" fn my_host_function(value: i32) -> i32 {
//!     value * 2
//! }
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
mod os;
mod reader;
mod relocation;
pub mod segment;
mod symbol;
#[cfg(feature = "version")]
mod version;

use alloc::borrow::Cow;
use core::fmt::{Debug, Display};

pub use elf::abi;
pub use format::{
    DylibImage, ElfImage, ElfModule, ElfModuleRef, ExecImage, LoadedDylib, LoadedExec,
    LoadedModule, Symbol,
};
pub use loader::{LoadHook, LoadHookContext, Loader};
pub use reader::{ElfBinary, ElfFile, ElfReader};
pub use relocation::{RelocationContext, RelocationHandler, SymbolLookup};

/// Error types used throughout the `elf_loader` library.
///
/// These errors represent various failure conditions that can occur during
/// ELF file loading, parsing, and relocation operations.
#[derive(Debug)]
pub enum Error {
    /// An error occurred while opening, reading, or writing ELF files.
    ///
    /// This error typically indicates issues with file I/O operations such as:
    /// * File not found
    /// * Permission denied
    /// * I/O errors during read/write operations
    Io {
        /// A descriptive message about the I/O error.
        msg: Cow<'static, str>,
    },

    /// An error occurred during memory mapping operations.
    ///
    /// This error typically indicates issues with memory management operations such as:
    /// * Failed to map file into memory
    /// * Failed to change memory protection
    /// * Failed to unmap memory regions
    Mmap {
        /// A descriptive message about the memory mapping error.
        msg: Cow<'static, str>,
    },

    /// An error occurred during dynamic library relocation.
    ///
    /// This error typically indicates issues with symbol resolution or relocation
    /// operations such as:
    /// * Undefined symbols
    /// * Incompatible symbol types
    /// * Relocation calculation errors
    Relocation {
        /// A descriptive message about the relocation error.
        msg: Cow<'static, str>,
    },

    /// An error occurred while parsing the dynamic section.
    ///
    /// This error typically indicates issues with parsing the `.dynamic` section such as:
    /// * Invalid dynamic entry types
    /// * Missing required dynamic entries
    /// * Malformed dynamic section data
    ParseDynamic {
        /// A descriptive message about the dynamic section parsing error.
        msg: Cow<'static, str>,
    },

    /// An error occurred while parsing the ELF header.
    ///
    /// This error typically indicates issues with the ELF header such as:
    /// * Invalid magic bytes
    /// * Unsupported ELF class or data encoding
    /// * Invalid ELF header fields
    ParseEhdr {
        /// A descriptive message about the ELF header parsing error.
        msg: Cow<'static, str>,
    },

    /// An error occurred while parsing program headers.
    ///
    /// This error typically indicates issues with program header parsing such as:
    /// * Invalid program header types
    /// * Malformed program header data
    /// * Incompatible program header entries
    ParsePhdr {
        /// A descriptive message about the program header parsing error.
        msg: Cow<'static, str>,
    },

    /// An error occurred in a user-defined callback or handler.
    Custom {
        /// A descriptive message about the custom error.
        msg: Cow<'static, str>,
    },
}

impl Display for Error {
    /// Formats the error for display purposes.
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

/// Creates an I/O error with the specified message.
///
/// This is a convenience function for creating `Error::Io` variants.
///
/// # Arguments
/// * `msg` - The error message.
///
/// # Returns
/// An `Error::Io` variant with the specified message.
#[cold]
#[inline(never)]
#[allow(unused)]
fn io_error(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::Io { msg: msg.into() }
}

/// Creates a relocation error with the specified message.
///
/// This is a convenience function for creating `Error::Relocation` variants.
///
/// # Arguments
/// * `msg` - The error message.
///
/// # Returns
/// An `Error::Relocation` variant with the specified message.
#[cold]
#[inline(never)]
fn relocate_error(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::Relocation { msg: msg.into() }
}

/// Creates a dynamic section parsing error with the specified message.
///
/// This is a convenience function for creating `Error::ParseDynamic` variants.
///
/// # Arguments
/// * `msg` - The error message.
///
/// # Returns
/// An `Error::ParseDynamic` variant with the specified message.
#[cold]
#[inline(never)]
fn parse_dynamic_error(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::ParseDynamic { msg: msg.into() }
}

/// Creates an ELF header parsing error with the specified message.
///
/// This is a convenience function for creating `Error::ParseEhdr` variants.
///
/// # Arguments
/// * `msg` - The error message.
///
/// # Returns
/// An `Error::ParseEhdr` variant with the specified message.
#[cold]
#[inline(never)]
fn parse_ehdr_error(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::ParseEhdr { msg: msg.into() }
}

/// Creates a custom error with the specified message.
///
/// This is a convenience function for creating `Error::Custom` variants.
///
/// # Arguments
/// * `msg` - The error message.
///
/// # Returns
/// An `Error::Custom` variant with the specified message.
#[cold]
#[inline(never)]
#[allow(unused)]
pub fn custom_error(msg: impl Into<Cow<'static, str>>) -> Error {
    Error::Custom { msg: msg.into() }
}

/// A type alias for `Result`s returned by `elf_loader` functions.
///
/// This is a convenience alias that eliminates the need to repeatedly specify
/// the `Error` type in function signatures.
pub type Result<T> = core::result::Result<T, Error>;
