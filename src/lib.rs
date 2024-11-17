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

use alloc::{
    boxed::Box,
    ffi::CString,
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use arch::{Dyn, Phdr};
use core::{
    any::Any,
    ffi::CStr,
    fmt::{Debug, Display},
    marker::PhantomData,
    ops::{self, Range},
};
use dynamic::ElfDynamic;

use object::*;
use relocation::ElfRelocation;
use segment::{ELFRelro, ElfSegments};
use symbol::{SymbolData, SymbolInfo};

pub use elf::abi;
pub use loader::Loader;

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
    phdrs: &'static [Phdr],
    /// entry
    entry: usize,
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
    pub fn entry(&self) -> usize {
        self.entry + self.base()
    }

    #[inline]
    pub fn phdrs(&self) -> &[Phdr] {
        self.phdrs
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

#[allow(unused)]
pub(crate) struct RelocatedInner {
    name: CString,
    entry: usize,
    base: usize,
    symbols: SymbolData,
    dynamic: *const Dyn,
    #[cfg(feature = "tls")]
    tls: Option<usize>,
    /// .fini
    fini_fn: Option<extern "C" fn()>,
    /// .fini_array
    fini_array_fn: Option<&'static [extern "C" fn()]>,
    /// user data
    user_data: UserData,
    /// semgents
    segments: ElfSegments,
    /// dependency libraries
    dep_libs: Vec<RelocatedDylib>,
}

impl Debug for RelocatedInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RelocatedLibrary")
            .field("name", &self.name)
            .field("base", &self.base)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct Symbol<'lib, T: 'lib> {
    ptr: *mut (),
    pd: PhantomData<&'lib T>,
}

impl<'lib, T> ops::Deref for Symbol<'lib, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*(&self.ptr as *const *mut _ as *const T) }
    }
}

#[derive(Clone)]
pub struct RelocatedDylib {
    pub(crate) inner: Arc<RelocatedInner>,
}

impl Debug for RelocatedDylib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

unsafe impl Send for RelocatedDylib {}
unsafe impl Sync for RelocatedDylib {}

impl RelocatedDylib {
    /// Retrieves the list of dependent libraries.
    ///
    /// This method returns an optional reference to a vector of `RelocatedDylib` instances,
    /// which represent the libraries that the current dynamic library depends on.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// if let Some(dependencies) = library.dep_libs() {
    ///     for lib in dependencies {
    ///         println!("Dependency: {:?}", lib);
    ///     }
    /// } else {
    ///     println!("No dependencies found.");
    /// }
    /// ```
    pub fn dep_libs(&self) -> Option<&Vec<RelocatedDylib>> {
        if self.inner.dep_libs.is_empty() {
            None
        } else {
            Some(&self.inner.dep_libs)
        }
    }

    /// Retrieves the name of the dynamic library.
    ///
    /// This method returns a string slice that represents the name of the dynamic library.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let library_name = library.name();
    /// println!("The dynamic library name is: {}", library_name);
    /// ```
    #[inline]
    pub fn name(&self) -> &str {
        self.inner.name.to_str().unwrap()
    }

    #[inline]
    pub fn cname(&self) -> &CStr {
        &self.inner.name
    }

    #[inline]
    pub fn base(&self) -> usize {
        self.inner.base
    }

    #[inline]
    pub fn user_data(&self) -> &UserData {
        &self.inner.user_data
    }

    #[inline]
    pub fn entry(&self) -> usize {
        self.base() + self.inner.entry
    }

    #[allow(unused_variables)]
    pub unsafe fn from_raw(
        name: CString,
        entry: usize,
        base: usize,
        dynamic: ElfDynamic,
        tls: Option<usize>,
        segments: ElfSegments,
        user_data: UserData,
    ) -> Self {
        Self {
            inner: Arc::new(RelocatedInner {
                name,
                entry,
                base,
                symbols: SymbolData::new(&dynamic),
                dynamic: dynamic.dyn_ptr,
                #[cfg(feature = "tls")]
                tls,
                segments,
                fini_fn: None,
                fini_array_fn: None,
                user_data: UserData::empty(),
                dep_libs: Vec::new(),
            }),
        }
    }

    /// Get a pointer to a function or static variable by symbol name.
    ///
    /// The symbol is interpreted as-is; no mangling is done. This means that symbols like `x::y` are
    /// most likely invalid.
    ///
    /// # Safety
    ///
    /// Users of this API must specify the correct type of the function or variable loaded.
    ///
    ///
    /// # Examples
    ///
    /// Given a loaded library:
    ///
    /// ```no_run
    /// # use ::dlopen_rs::ELFLibrary;
    /// let lib = ELFLibrary::from_file("/path/to/awesome.module")
    ///		.unwrap()
    ///		.relocate(&[])
    ///		.unwrap();
    /// ```
    ///
    /// Loading and using a function looks like this:
    ///
    /// ```no_run
    /// unsafe {
    ///     let awesome_function: Symbol<unsafe extern fn(f64) -> f64> =
    ///         lib.get("awesome_function").unwrap();
    ///     awesome_function(0.42);
    /// }
    /// ```
    ///
    /// A static variable may also be loaded and inspected:
    ///
    /// ```no_run
    /// unsafe {
    ///     let awesome_variable: Symbol<*mut f64> = lib.get("awesome_variable").unwrap();
    ///     **awesome_variable = 42.0;
    /// };
    /// ```
    pub unsafe fn get<'lib, T>(&'lib self, name: &str) -> Result<Symbol<'lib, T>> {
        self.inner
            .symbols
            .get_sym(&SymbolInfo::new(name))
            .map(|sym| Symbol {
                ptr: (self.base() + sym.st_value as usize) as _,
                pd: PhantomData,
            })
            .ok_or(find_symbol_error(format!("can not find symbol:{}", name)))
    }

    /// Attempts to load a versioned symbol from the dynamically-linked library.
    ///
    /// # Safety
    /// This function is unsafe because it involves raw pointer manipulation and
    /// dereferencing. The caller must ensure that the library handle is valid
    /// and that the symbol exists and has the correct type.
    ///
    /// # Parameters
    /// - `&'lib self`: A reference to the library instance from which the symbol will be loaded.
    /// - `name`: The name of the symbol to load.
    /// - `version`: The version of the symbol to load.
    ///
    /// # Returns
    /// If the symbol is found and has the correct type, this function returns
    /// `Ok(Symbol<'lib, T>)`, where `Symbol` is a wrapper around a raw function pointer.
    /// If the symbol cannot be found or an error occurs, it returns an `Err` with a message.
    ///
    /// # Examples
    /// ```
    /// let symbol = unsafe { lib.get_version::<fn()>>("function_name", "1.0").unwrap() };
    /// ```
    ///
    /// # Errors
    /// Returns a custom error if the symbol cannot be found, or if there is a problem
    /// retrieving the symbol.
    #[cfg(feature = "version")]
    pub unsafe fn get_version<'lib, T>(
        &'lib self,
        name: &str,
        version: &str,
    ) -> Result<Symbol<'lib, T>> {
        self.inner
            .symbols
            .get_sym(&SymbolInfo::new_with_version(name, version))
            .map(|sym| Symbol {
                ptr: (self.base() + sym.st_value as usize) as _,
                pd: PhantomData,
            })
            .ok_or(find_symbol_error(format!("can not find symbol:{}", name)))
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
