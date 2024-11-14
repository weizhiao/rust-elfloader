#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "x86",
    target_arch = "aarch64",
    target_arch = "riscv64",
)))]
compile_error!("unsupport arch");

pub mod arch;
pub mod dynamic;
mod loader;
pub mod mmap;
pub mod object;
pub mod relocation;
pub mod segment;
mod symbol;
#[cfg(feature = "version")]
mod version;

use alloc::{boxed::Box, ffi::CString, string::String, string::ToString, vec::Vec};
use arch::{Dyn, Phdr};
use core::{
    any::Any,
    ffi::CStr,
    fmt::{Debug, Display},
    marker::PhantomData,
    ops::Range,
};

use object::*;
use relocation::{ElfRelocation, RelocatedDylib};
use segment::{ELFRelro, ElfSegments};
use symbol::SymbolData;

pub use loader::Loader;
pub use relocation::Symbol;

impl<T: ThreadLocal, U: Unwind> Debug for ElfDylib<T, U> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ELFLibrary")
            .field("name", &self.name)
            .field("needed_libs", &self.needed_libs)
            .finish()
    }
}

pub trait Unwind: Sized + 'static {
    unsafe fn new(phdr: &Phdr, map_range: Range<usize>) -> Option<Self>;
}

pub trait ThreadLocal: Sized + 'static {
    unsafe fn new(phdr: &Phdr, base: usize) -> Option<Self>;
    unsafe fn module_id(&self) -> usize;
}

pub struct UserData {
    data: Vec<Box<dyn Any + 'static>>,
}

impl UserData {
    pub const fn empty() -> Self {
        Self { data: Vec::new() }
    }

    #[inline]
    pub fn data_mut(&mut self) -> &mut Vec<Box<dyn Any + 'static>> {
        &mut self.data
    }

    #[inline]
    pub fn data(&self) -> &[Box<dyn Any>] {
        &self.data
    }
}

pub struct ElfDylib<T, U>
where
    T: ThreadLocal,
    U: Unwind,
{
    /// file name
    name: CString,
    /// phdr
    phdrs: Vec<Phdr>,
    /// elf symbols
    symbols: SymbolData,
    /// dynamic
    dynamic: *const Dyn,
    #[cfg(feature = "tls")]
    /// .tbss and .tdata
    tls: Option<T>,
    /// .eh_frame
    unwind: Option<U>,
    /// semgents
    segments: ElfSegments,
    /// .fini
    fini_fn: Option<extern "C" fn()>,
    /// .fini_array
    fini_array_fn: Option<&'static [extern "C" fn()]>,
    /// user data
    user_data: UserData,
    /// dependency libraries
    dep_libs: Vec<RelocatedDylib>,
    /// rela.dyn and rela.plt
    relocation: ElfRelocation,
    /// GNU_RELRO segment
    relro: Option<ELFRelro>,
    /// .init
    init_fn: Option<extern "C" fn()>,
    /// .init_array
    init_array_fn: Option<&'static [extern "C" fn()]>,
    /// needed libs' name
    needed_libs: Vec<&'static str>,
    _marker: PhantomData<T>,
}

impl<T: ThreadLocal, U: Unwind> ElfDylib<T, U> {
    #[inline]
    pub fn phdrs(&self) -> &[Phdr] {
        &self.phdrs
    }

    #[inline]
    pub fn cname(&self) -> &CStr {
        self.name.as_c_str()
    }

    #[inline]
    pub fn name(&self) -> &str {
        self.name.to_str().unwrap()
    }

    #[inline]
    pub fn dynamic(&self) -> *const Dyn {
        self.dynamic
    }

    #[inline]
    pub fn base(&self) -> usize {
        self.segments.base()
    }

    #[inline]
    pub unsafe fn user_data_mut(&mut self) -> &mut UserData {
        &mut self.user_data
    }

    #[inline]
    pub fn user_data(&self) -> &UserData {
        &self.user_data
    }
}

#[derive(Debug)]
pub enum Error {
    /// Returned when encountered an io error.
    #[cfg(feature = "std")]
    IOError {
        err: std::io::Error,
    },
    MmapError {
        msg: String,
    },
    RelocateError {
        msg: String,
    },
    FindSymbolError {
        msg: String,
    },
    ParseDynamicError {
        msg: &'static str,
    },
    ParseEhdrError {
        msg: String,
    },
    UnKnownError {
        msg: String,
    },
}

impl Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            #[cfg(feature = "std")]
            Error::IOError { err } => write!(f, "{err}"),
            Error::MmapError { msg } => write!(f, "{msg}"),
            Error::RelocateError { msg } => write!(f, "{msg}"),
            Error::FindSymbolError { msg } => write!(f, "{msg}"),
            Error::ParseDynamicError { msg } => write!(f, "{msg}"),
            Error::ParseEhdrError { msg } => write!(f, "{msg}"),
            Error::UnKnownError { msg } => write!(f, "{msg}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::IOError { err } => Some(err),
            _ => None,
        }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    #[cold]
    fn from(value: std::io::Error) -> Self {
        Error::IOError { err: value }
    }
}

#[cold]
#[inline(never)]
fn relocate_error(msg: impl ToString) -> Error {
    Error::RelocateError {
        msg: msg.to_string(),
    }
}

#[cold]
#[inline(never)]
fn find_symbol_error(msg: impl ToString) -> Error {
    Error::FindSymbolError {
        msg: msg.to_string(),
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

pub type Result<T> = core::result::Result<T, Error>;
