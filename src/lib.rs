//! # elf_loader
//! A `safe`, `lightweight`, `extensible`, and `high-performance` library for loading ELF files.
//! ## Usage
//! It implements the general steps for loading ELF files and leaves extension interfaces,
//! allowing users to implement their own customized loaders.
//! ## Example
//! This repository provides an example of a [mini-loader](https://github.com/weizhiao/elf_loader/tree/main/mini-loader) implemented using `elf_loader`.
//! The miniloader can load PIE files and currently only supports `x86_64`.
#![no_std]
extern crate alloc;

#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "riscv64",
)))]
compile_error!("unsupport arch");

pub mod arch;
pub mod dynamic;
mod loader;
pub mod mmap;
pub mod object;
mod relocation;
pub mod segment;
mod symbol;
#[cfg(feature = "version")]
mod version;

use alloc::{
    boxed::Box,
    ffi::CString,
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use arch::{Dyn, ElfRela, Phdr};
use core::{
    any::Any,
    ffi::CStr,
    fmt::{Debug, Display},
    marker::PhantomData,
    ops::{self, Deref},
    sync::atomic::{AtomicBool, Ordering},
};
use dynamic::ElfDynamic;

use object::*;
use relocation::{ElfRelocation, GLOBAL_SCOPE};
use segment::{ELFRelro, ElfSegments};
use symbol::{SymbolInfo, SymbolTable};

pub use elf::abi;
pub use loader::Loader;

impl Debug for ElfDylib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ElfLibrary")
            .field("name", &self.core.inner.name)
            .field("needed_libs", &self.core.inner.needed_libs)
            .finish()
    }
}

struct DataItem {
    key: u8,
    value: Option<Box<dyn Any>>,
}

/// User-defined data associated with the loaded ELF file
pub struct UserData {
    data: Vec<DataItem>,
}

impl UserData {
    #[inline]
    pub const fn empty() -> Self {
        Self { data: Vec::new() }
    }

    #[inline]
    pub fn insert(&mut self, key: u8, value: Box<dyn Any>) -> Option<Box<dyn Any>> {
        for item in self.data.iter_mut() {
            if item.key == key {
                let old = core::mem::take(&mut item.value);
                item.value = Some(value);
                return old;
            }
        }
        self.data.push(DataItem {
            key,
            value: Some(value),
        });
        None
    }

    #[inline]
    pub fn get(&self, key: u8) -> Option<&Box<dyn Any>> {
        self.data.iter().find_map(|item| {
            if item.key == key {
                return item.value.as_ref();
            }
            None
        })
    }
}

#[derive(Clone, Copy)]
struct InitParams {
    argc: usize,
    argv: usize,
    envp: usize,
}

impl Deref for ElfDylib {
    type Target = CoreComponent;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl Deref for RelocatedDylib<'_> {
    type Target = CoreComponent;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

/// An unrelocated dynamic library
pub struct ElfDylib {
    /// entry
    entry: usize,
    /// .got.plt
    got: Option<*mut usize>,
    /// rela.dyn and rela.plt
    relocation: ElfRelocation,
    /// GNU_RELRO segment
    relro: Option<ELFRelro>,
    /// init params
    init_params: Option<InitParams>,
    /// .init
    init_fn: Option<extern "C" fn()>,
    /// .init_array
    init_array_fn: Option<&'static [extern "C" fn()]>,
    /// lazy binding
    lazy: bool,
    /// DT_RPATH
    rpath: Option<&'static str>,
    /// DT_RUNPATH
    runpath: Option<&'static str>,
    /// core component
    core: CoreComponent,
}

impl ElfDylib {
    /// Gets the entry point of the elf object.
    #[inline]
    pub fn entry(&self) -> usize {
        self.entry + self.base()
    }

    /// Gets the core component reference of the elf object
    #[inline]
    pub fn core_component_ref(&self) -> &CoreComponent {
        &self.core
    }

    /// Gets the core component of the elf object
    #[inline]
    pub fn core_component(&self) -> CoreComponent {
        self.core.clone()
    }

    /// Gets mutable user data from the elf object.
    #[inline]
    pub fn user_data_mut(&mut self) -> Option<&mut UserData> {
        Arc::get_mut(&mut self.core.inner).map(|inner| &mut inner.user_data)
    }

    /// Whether lazy binding is enabled for the current dynamic library.
    #[inline]
    pub fn is_lazy(&self) -> bool {
        self.lazy
    }

    /// Gets the DT_RPATH value.
    #[inline]
    pub fn rpath(&self) -> Option<&str> {
        self.rpath
    }

    /// Gets the DT_RUNPATH value.
    #[inline]
    pub fn runpath(&self) -> Option<&str> {
        self.runpath
    }
}

struct CoreComponentInner {
    /// is initialized
    is_init: AtomicBool,
    /// file name
    name: CString,
    /// elf symbols
    symbols: SymbolTable,
    /// dynamic
    dynamic: *const Dyn,
    /// rela.plt
    pltrel: *const ElfRela,
    /// phdrs
    phdrs: &'static [Phdr],
    /// .fini
    fini_fn: Option<extern "C" fn()>,
    /// .fini_array
    fini_array_fn: Option<&'static [extern "C" fn()]>,
    /// needed libs' name
    needed_libs: Box<[&'static str]>,
    /// user data
    user_data: UserData,
    /// lazy binding scope
    lazy_scope: Option<Box<dyn Fn(&str) -> Option<*const ()> + 'static>>,
    /// semgents
    segments: ElfSegments,
}

impl Drop for CoreComponentInner {
    fn drop(&mut self) {
        if self.is_init.load(Ordering::Relaxed) {
            self.fini_fn
                .iter()
                .chain(self.fini_array_fn.unwrap_or(&[]).iter())
                .for_each(|fini| fini());
        }
    }
}

/// `CoreComponentRef` is a version of `CoreComponent` that holds a non-owning reference to the managed allocation.
pub struct CoreComponentRef {
    inner: Weak<CoreComponentInner>,
}

impl CoreComponentRef {
    /// Attempts to upgrade the Weak pointer to an Arc
    pub fn upgrade(&self) -> Option<CoreComponent> {
        self.inner.upgrade().map(|inner| CoreComponent { inner })
    }
}

/// The core part of an elf object
#[derive(Clone)]
pub struct CoreComponent {
    inner: Arc<CoreComponentInner>,
}

unsafe impl Sync for CoreComponent {}
unsafe impl Send for CoreComponent {}

impl CoreComponent {
    #[inline]
    pub(crate) fn set_lazy_scope(
        &self,
        lazy_scope: Option<Box<dyn Fn(&str) -> Option<*const ()> + 'static>>,
    ) {
        // 因为在完成重定位前，只有unsafe的方法可以拿到CoreComponent的引用，所以这里认为是安全的
        let ptr = unsafe { &mut *(Arc::as_ptr(&self.inner) as *mut CoreComponentInner) };
        ptr.lazy_scope = lazy_scope;
    }

    #[inline]
    pub(crate) fn set_init(&self) {
        self.inner.is_init.store(true, Ordering::Relaxed);
    }

    #[inline]
    /// Creates a new Weak pointer to this allocation.
    pub fn downgrade(&self) -> CoreComponentRef {
        CoreComponentRef {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// Gets user data from the elf object.
    #[inline]
    pub fn user_data(&self) -> &UserData {
        &self.inner.user_data
    }

    /// Gets the number of strong references to the elf object.
    #[inline]
    pub fn strong_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    /// Gets the number of weak references to the elf object.
    #[inline]
    pub fn weak_count(&self) -> usize {
        Arc::weak_count(&self.inner)
    }

    /// Gets the name of the elf object.
    #[inline]
    pub fn name(&self) -> &str {
        self.inner.name.to_str().unwrap()
    }

    /// Gets the C-style name of the elf object.
    #[inline]
    pub fn cname(&self) -> &CStr {
        &self.inner.name
    }

    /// Gets the short name of the elf object.
    #[inline]
    pub fn shortname(&self) -> &str {
        self.name().split('/').last().unwrap()
    }

    /// Gets the base address of the elf object.
    #[inline]
    pub fn base(&self) -> usize {
        self.inner.segments.base()
    }

    /// Gets the memory length of the elf object map.
    #[inline]
    pub fn map_len(&self) -> usize {
        self.inner.segments.len()
    }

    /// Gets the program headers of the elf object.
    #[inline]
    pub fn phdrs(&self) -> &[Phdr] {
        &self.inner.phdrs
    }

    /// Gets the address of the dynamic section.
    #[inline]
    pub fn dynamic(&self) -> *const Dyn {
        self.inner.dynamic
    }

    /// Gets the needed libs' name of the elf object.
    #[inline]
    pub fn needed_libs(&self) -> &[&'static str] {
        &self.inner.needed_libs
    }

    /// Gets the symbol table.
    #[inline]
    pub fn symtab(&self) -> &SymbolTable {
        &self.inner.symbols
    }

    fn from_raw(
        name: CString,
        base: usize,
        dynamic: ElfDynamic,
        phdrs: &'static [Phdr],
        mut segments: ElfSegments,
        user_data: UserData,
    ) -> Self {
        segments.offset = (segments.memory.as_ptr() as usize).wrapping_sub(base);
        Self {
            inner: Arc::new(CoreComponentInner {
                name,
                is_init: AtomicBool::new(true),
                symbols: SymbolTable::new(&dynamic),
                pltrel: core::ptr::null(),
                dynamic: dynamic.dyn_ptr,
                phdrs,
                segments,
                fini_fn: None,
                fini_array_fn: None,
                needed_libs: Box::new([]),
                user_data,
                lazy_scope: None,
            }),
        }
    }
}

impl Debug for CoreComponent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dylib")
            .field("name", &self.inner.name)
            .finish()
    }
}

/// A symbol from elf object
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

impl<'lib, T> Symbol<'lib, T> {
    pub fn into_raw(self) -> *const () {
        self.ptr
    }
}

/// A dynamic library that has been relocated
#[derive(Clone)]
pub struct RelocatedDylib<'scope> {
    core: CoreComponent,
    _marker: PhantomData<&'scope ()>,
}

impl<'scope> Debug for RelocatedDylib<'scope> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.core.fmt(f)
    }
}

impl<'scope> RelocatedDylib<'scope> {
    /// # Safety
    /// The current elf object has not yet been relocated, so it is dangerous to use this
    /// function to convert `CoreComponent` to `RelocateDylib`. And lifecycle information is lost
    #[inline]
    pub unsafe fn from_core_component(core: CoreComponent) -> Self {
        RelocatedDylib {
            core,
            _marker: PhantomData,
        }
    }

    /// Gets the core component reference of the elf object.
    /// # Safety
    /// Lifecycle information is lost, and the dependencies of the current elf object can be prematurely deallocated,
    /// which can cause serious problems.
    #[inline]
    pub unsafe fn core_component_ref(&self) -> &CoreComponent {
        &self.core
    }

    #[inline]
    pub unsafe fn new_uncheck(
        name: CString,
        base: usize,
        dynamic: ElfDynamic,
        phdrs: &'static [Phdr],
        segments: ElfSegments,
        user_data: UserData,
    ) -> Self {
        Self {
            core: CoreComponent::from_raw(name, base, dynamic, phdrs, segments, user_data),
            _marker: PhantomData,
        }
    }

    /// Gets a pointer to a function or static variable by symbol name.
    ///
    /// The symbol is interpreted as-is; no mangling is done. This means that symbols like `x::y` are
    /// most likely invalid.
    ///
    /// # Safety
    /// Users of this API must specify the correct type of the function or variable loaded.
    ///
    /// # Examples
    /// ```no_run
    /// unsafe {
    ///     let awesome_function: Symbol<unsafe extern fn(f64) -> f64> =
    ///         lib.get("awesome_function").unwrap();
    ///     awesome_function(0.42);
    /// }
    /// ```
    /// A static variable may also be loaded and inspected:
    /// ```no_run
    /// unsafe {
    ///     let awesome_variable: Symbol<*mut f64> = lib.get("awesome_variable").unwrap();
    ///     **awesome_variable = 42.0;
    /// };
    /// ```
    #[inline]
    pub unsafe fn get<'lib, T>(&'lib self, name: &str) -> Option<Symbol<'lib, T>> {
        self.symtab()
            .lookup_filter(&SymbolInfo::from_str(name))
            .map(|sym| Symbol {
                ptr: (self.base() + sym.st_value()) as _,
                pd: PhantomData,
            })
    }

    /// Load a versioned symbol from the elf object.
    ///
    /// # Examples
    /// ```
    /// let symbol = unsafe { lib.get_version::<fn()>>("function_name", "1.0").unwrap() };
    /// ```
    #[cfg(feature = "version")]
    #[inline]
    pub unsafe fn get_version<'lib, T>(
        &'lib self,
        name: &str,
        version: &str,
    ) -> Option<Symbol<'lib, T>> {
        self.symtab()
            .lookup_filter(&SymbolInfo::new_with_version(name, version))
            .map(|sym| Symbol {
                ptr: (self.base() + sym.st_value()) as _,
                pd: PhantomData,
            })
    }
}

/// elf_loader error types
#[derive(Debug)]
pub enum Error {
    /// An error occurred while opening or reading or writing elf files.
    #[cfg(feature = "fs")]
    IOError { msg: String },
    /// An error occurred while memory mapping.
    MmapError { msg: String },
    /// An error occurred during dynamic library relocation.
    RelocateError {
        msg: String,
        custom_err: Box<dyn Any>,
    },
    /// An error occurred while parsing dynamic section.
    ParseDynamicError { msg: &'static str },
    /// An error occurred while parsing elf header.
    ParseEhdrError { msg: String },
    /// An error occurred while parsing program header.
    ParsePhdrError {
        msg: String,
        custom_err: Box<dyn Any>,
    },
}

impl Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            #[cfg(feature = "fs")]
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

#[cfg(feature = "fs")]
#[cold]
#[inline(never)]
fn io_error(msg: impl ToString) -> Error {
    Error::IOError {
        msg: msg.to_string(),
    }
}

#[cold]
#[inline(never)]
fn relocate_error(msg: impl ToString, custom_err: Box<dyn Any>) -> Error {
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
fn parse_phdr_error(msg: impl ToString, custom_err: Box<dyn Any>) -> Error {
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
///        If the symbol is found in the global scope, the function should return `Some(raw_pointer)`,
///        otherwise, it should return `None`.
///
/// # Return
/// This function does not return a value. It sets the global scope for lazy binding.
pub unsafe fn set_global_scope(f: fn(&str) -> Option<*const ()>) {
    GLOBAL_SCOPE.store(f as usize, core::sync::atomic::Ordering::Release);
}

pub type Result<T> = core::result::Result<T, Error>;
