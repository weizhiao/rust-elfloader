//! # elf_loader
//! A `safe`, `lightweight`, `extensible`, and `high-performance` library for loading ELF files.
//! ## Usage
//! `elf_loader` can load various ELF files and provides interfaces for extended functionality. It can be used in the following areas:
//! * Use it as an ELF file loader in operating system kernels
//! * Use it to implement a Rust version of the dynamic linker
//! * Use it to load ELF dynamic libraries on embedded devices
//! ## Example
//! ```rust, ignore
//! use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
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
//! let mut loader = Loader::<MmapImpl>::new();
//! let liba = loader
//!     .easy_load_dylib(ElfFile::from_path("target/liba.so").unwrap())
//!     .unwrap();
//!     // Relocate symbols in liba.so
//! let a = liba.easy_relocate([].iter(), &pre_find).unwrap();
//! // Call function a in liba.so
//! let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
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

#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "riscv64",
    target_arch = "riscv32",
    target_arch = "loongarch64",
    target_arch = "x86",
    target_arch = "arm",
)))]
compile_error!("unsupport arch");

pub mod arch;
pub mod dynamic;
mod format;
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

use alloc::{
    boxed::Box,
    string::{String, ToString},
};
use core::{
    any::Any,
    fmt::{Debug, Display},
};
use object::*;
use relocation::{ElfRelocation, GLOBAL_SCOPE};
use segment::ELFRelro;

pub use elf::abi;
pub use format::dylib::{ElfDylib, RelocatedDylib, Symbol};
pub use format::exec::{ElfExec, RelocatedExec};
pub use format::{CoreComponent, CoreComponentRef, Elf, UserData};
pub use loader::Loader;
pub use relocation::find_symdef;

/// elf_loader error types
#[derive(Debug)]
pub enum Error {
    /// An error occurred while opening or reading or writing elf files.
    IOError { msg: String },
    /// An error occurred while memory mapping.
    MmapError { msg: String },
    /// An error occurred during dynamic library relocation.
    RelocateError {
        msg: String,
        custom_err: Box<dyn Any + Send + Sync>,
    },
    /// An error occurred while parsing dynamic section.
    ParseDynamicError { msg: &'static str },
    /// An error occurred while parsing elf header.
    ParseEhdrError { msg: String },
    /// An error occurred while parsing program header.
    ParsePhdrError {
        msg: String,
        custom_err: Box<dyn Any + Send + Sync>,
    },
}

impl Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::IOError { msg } => write!(f, "{msg}"),
            Error::MmapError { msg } => write!(f, "{msg}"),
            Error::RelocateError { msg, .. } => write!(f, "{msg}"),
            Error::ParseDynamicError { msg } => write!(f, "{msg}"),
            Error::ParseEhdrError { msg } => write!(f, "{msg}"),
            Error::ParsePhdrError { msg, .. } => write!(f, "{msg}"),
        }
    }
}

impl core::error::Error for Error {}

#[cold]
#[inline(never)]
#[allow(unused)]
fn io_error(msg: impl ToString) -> Error {
    Error::IOError {
        msg: msg.to_string(),
    }
}

#[cold]
#[inline(never)]
fn relocate_error(msg: impl ToString, custom_err: Box<dyn Any + Send + Sync>) -> Error {
    Error::RelocateError {
        msg: msg.to_string(),
        custom_err,
    }
}

#[cold]
#[inline(never)]
fn parse_dynamic_error(msg: &'static str) -> Error {
    Error::ParseDynamicError { msg }
}

#[cold]
#[inline(never)]
fn parse_ehdr_error(msg: impl ToString) -> Error {
    Error::ParseEhdrError {
        msg: msg.to_string(),
    }
}

#[cold]
#[inline(never)]
fn parse_phdr_error(msg: impl ToString, custom_err: Box<dyn Any + Send + Sync>) -> Error {
    Error::ParsePhdrError {
        msg: msg.to_string(),
        custom_err,
    }
}

/// Set the global scope, lazy binding will look for the symbol in the global scope.
///
/// # Safety
/// This function is marked as unsafe because it directly interacts with raw pointers,
/// and it also requires the user to ensure thread safety.  
/// It is the caller's responsibility to ensure that the provided function `f` behaves correctly.
///
/// # Parameters
/// - `f`: A function that takes a symbol name as a parameter and returns an optional raw pointer.
///   If the symbol is found in the global scope, the function should return `Some(raw_pointer)`,
///   otherwise, it should return `None`.
///
/// # Return
/// This function does not return a value. It sets the global scope for lazy binding.
pub unsafe fn set_global_scope(f: fn(&str) -> Option<*const ()>) {
    GLOBAL_SCOPE.store(f as usize, core::sync::atomic::Ordering::Release);
}

pub type Result<T> = core::result::Result<T, Error>;
