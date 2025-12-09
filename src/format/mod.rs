//! ELF file format handling
//!
//! This module provides the core data structures and functionality for working
//! with ELF files in various stages of processing: from raw ELF files to
//! relocated and loaded libraries or executables.

pub(crate) mod relocatable;
pub(crate) mod relocated;

use crate::{
    Loader, Result,
    arch::{Dyn, ElfPhdr, ElfRelType},
    dynamic::ElfDynamic,
    format::relocated::{ElfDylib, ElfExec, RelocatedCommonPart, RelocatedDylib, RelocatedExec},
    loader::FnHandler,
    mmap::Mmap,
    object::ElfObject,
    relocation::{
        SymDef,
        dynamic_link::{LazyScope, UnknownHandler},
    },
    segment::ElfSegments,
    symbol::{SymbolInfo, SymbolTable},
};
use alloc::{boxed::Box, ffi::CString, vec::Vec};
use core::{
    any::Any,
    ffi::CStr,
    fmt::Debug,
    marker::PhantomData,
    ops::Deref,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::{Arc, Weak};
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::{Arc, Weak};

/// Internal data item for user-defined data storage
struct DataItem {
    /// Key identifier for the data item
    key: u8,

    /// Optional boxed value stored with this key
    value: Option<Box<dyn Any>>,
}

/// User-defined data associated with the loaded ELF file
///
/// This structure allows users to associate custom data with loaded ELF files.
/// It provides a simple key-value store where keys are bytes and values are
/// boxed any-type objects.
pub struct UserData {
    /// Vector of data items stored as key-value pairs
    data: Vec<DataItem>,
}

impl UserData {
    /// Creates an empty UserData instance
    ///
    /// # Returns
    /// A new, empty UserData instance
    #[inline]
    pub const fn empty() -> Self {
        Self { data: Vec::new() }
    }

    /// Inserts a key-value pair into the user data
    ///
    /// If a value with the same key already exists, it is replaced with the new value.
    ///
    /// # Arguments
    /// * `key` - The key to associate with the value
    /// * `value` - The boxed value to store
    ///
    /// # Returns
    /// * `Some(old_value)` - If a value with this key already existed
    /// * `None` - If this is a new key
    #[inline]
    pub fn insert(&mut self, key: u8, value: Box<dyn Any>) -> Option<Box<dyn Any>> {
        for item in &mut self.data {
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

    /// Retrieves a value by key from the user data
    ///
    /// # Arguments
    /// * `key` - The key of the value to retrieve
    ///
    /// # Returns
    /// * `Some(value)` - A reference to the boxed value if found
    /// * `None` - If no value with this key exists
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

impl Deref for Relocated<'_> {
    type Target = CoreComponent;

    /// Dereferences to the underlying CoreComponent
    ///
    /// This allows direct access to the CoreComponent fields through the Relocated wrapper.
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

/// An unrelocated ELF file
///
/// This enum represents an ELF file that has been loaded into memory but
/// has not yet undergone relocation. It can be either a dynamic library
/// or an executable.
#[derive(Debug)]
pub enum Elf {
    /// A dynamic library (shared object)
    Dylib(ElfDylib),

    /// An executable file
    Exec(ElfExec),
}

/// An ELF file that has been relocated
///
/// This enum represents an ELF file that has been loaded and relocated.
/// It maintains lifetime information to prevent premature deallocation
/// of dependencies.
#[derive(Debug, Clone)]
pub enum RelocatedElf<'scope> {
    /// A relocated dynamic library
    Dylib(RelocatedDylib<'scope>),

    /// A relocated executable
    Exec(RelocatedExec<'scope>),
}

impl<'scope> RelocatedElf<'scope> {
    /// Converts this RelocatedElf into a RelocatedDylib if it is one
    ///
    /// # Returns
    /// * `Some(dylib)` - If this is a Dylib variant
    /// * `None` - If this is an Exec variant
    #[inline]
    pub fn into_dylib(self) -> Option<RelocatedDylib<'scope>> {
        match self {
            RelocatedElf::Dylib(dylib) => Some(dylib),
            RelocatedElf::Exec(_) => None,
        }
    }

    /// Converts this RelocatedElf into a RelocatedExec if it is one
    ///
    /// # Returns
    /// * `Some(exec)` - If this is an Exec variant
    /// * `None` - If this is a Dylib variant
    #[inline]
    pub fn into_exec(self) -> Option<RelocatedExec<'scope>> {
        match self {
            RelocatedElf::Dylib(_) => None,
            RelocatedElf::Exec(exec) => Some(exec),
        }
    }

    /// Gets a reference to the RelocatedDylib if this is one
    ///
    /// # Returns
    /// * `Some(dylib)` - If this is a Dylib variant
    /// * `None` - If this is an Exec variant
    #[inline]
    pub fn as_dylib(&self) -> Option<&RelocatedDylib<'scope>> {
        match self {
            RelocatedElf::Dylib(dylib) => Some(dylib),
            RelocatedElf::Exec(_) => None,
        }
    }
}

impl Deref for Elf {
    type Target = RelocatedCommonPart;

    /// Dereferences to the underlying RelocatedCommonPart
    ///
    /// This allows direct access to common fields shared by all ELF file types.
    fn deref(&self) -> &Self::Target {
        match self {
            Elf::Dylib(elf_dylib) => elf_dylib,
            Elf::Exec(elf_exec) => elf_exec,
        }
    }
}

/// Creates a lazy scope for symbol resolution during lazy binding
///
/// This function creates a LazyScope that can be used during lazy binding
/// to resolve symbols. It searches through the provided libraries and
/// uses the pre_find function as a fallback.
///
/// # Arguments
/// * `libs` - Vector of CoreComponentRef instances to search for symbols
/// * `pre_find` - Function to use for initial symbol lookup
///
/// # Returns
/// A LazyScope that can be used for symbol resolution
pub(crate) fn create_lazy_scope<F>(libs: Vec<CoreComponentRef>, pre_find: &'_ F) -> LazyScope<'_>
where
    F: Fn(&str) -> Option<*const ()>,
{
    #[cfg(not(feature = "portable-atomic"))]
    type Ptr<T> = Arc<T>;
    #[cfg(feature = "portable-atomic")]
    type Ptr<T> = Box<T>;

    // workaround unstable CoerceUnsized by create Box<dyn _> then convert using Arc::from
    // https://github.com/rust-lang/rust/issues/18598
    let closure: Ptr<dyn for<'a> Fn(&'a str) -> Option<*const ()>> = Ptr::new(move |name| {
        libs.iter().find_map(|lib| {
            pre_find(name).or_else(|| unsafe {
                RelocatedDylib::from_core_component(lib.upgrade().unwrap())
                    .get::<()>(name)
                    .map(|sym| sym.into_raw())
            })
        })
    });
    closure.into()
}

impl Elf {
    /// Relocate the ELF file with the given dynamic libraries and function closure.
    ///
    /// During relocation, the symbol is first searched in the function closure `pre_find`.
    ///
    /// # Arguments
    /// * `scope` - Iterator over relocated libraries to use for symbol resolution
    /// * `pre_find` - Function to use for initial symbol lookup
    ///
    /// # Returns
    /// * `Ok(RelocatedElf)` - The relocated ELF file
    /// * `Err(Error)` - If relocation fails
    pub fn easy_relocate<'iter, 'scope, 'find, 'lib, F>(
        self,
        scope: impl IntoIterator<Item = &'iter Relocated<'scope>>,
        pre_find: &'find F,
    ) -> Result<RelocatedElf<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        match self {
            Elf::Dylib(elf_dylib) => Ok(RelocatedElf::Dylib(
                elf_dylib.easy_relocate(scope, pre_find)?,
            )),
            Elf::Exec(elf_exec) => Ok(RelocatedElf::Exec(elf_exec.easy_relocate(scope, pre_find)?)),
        }
    }

    /// Relocate the ELF file with the given dynamic libraries and function closure.
    ///
    /// This method provides more control over the relocation process compared to
    /// [easy_relocate], allowing custom handling of unknown relocations and
    /// specification of a local lazy scope.
    ///
    /// # Note
    /// * During relocation, the symbol is first searched in the function closure `pre_find`.
    /// * The `deal_unknown` function is used to handle relocation types not implemented by elf_loader or failed relocations
    /// * Relocation will be done in the exact order in which the dynamic libraries appear in `scope`.
    /// * When lazy binding, the symbol is first looked for in the global scope and then in the local lazy scope
    ///
    /// # Arguments
    /// * `scope` - Slice of relocated libraries to use for symbol resolution
    /// * `pre_find` - Function to use for initial symbol lookup
    /// * `deal_unknown` - Handler for unknown or failed relocations
    /// * `local_lazy_scope` - Optional local scope for lazy binding
    ///
    /// # Returns
    /// * `Ok(RelocatedElf)` - The relocated ELF file
    /// * `Err(Error)` - If relocation fails
    pub fn relocate<'iter, 'scope, 'find, 'lib, F>(
        self,
        scope: impl AsRef<[&'iter Relocated<'scope>]>,
        pre_find: &'find F,
        deal_unknown: &mut UnknownHandler,
        local_lazy_scope: Option<LazyScope<'lib>>,
    ) -> Result<RelocatedElf<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        let relocated_elf = match self {
            Elf::Dylib(elf_dylib) => RelocatedElf::Dylib(elf_dylib.relocate(
                scope,
                pre_find,
                deal_unknown,
                local_lazy_scope,
            )?),
            Elf::Exec(elf_exec) => RelocatedElf::Exec(elf_exec.relocate(
                scope,
                pre_find,
                deal_unknown,
                local_lazy_scope,
            )?),
        };
        Ok(relocated_elf)
    }
}

/// A symbol from ELF object
///
/// This structure represents a symbol loaded from an ELF file, such as a
/// function or global variable. It provides safe access to the symbol
/// while maintaining proper lifetime information.
#[derive(Debug, Clone)]
pub struct Symbol<'lib, T: 'lib> {
    /// Raw pointer to the symbol data
    pub(crate) ptr: *mut (),

    /// Phantom data to maintain lifetime information
    pd: PhantomData<&'lib T>,
}

impl<'lib, T> Deref for Symbol<'lib, T> {
    type Target = T;

    /// Dereferences to the underlying symbol type
    ///
    /// This allows direct use of the symbol as if it were of type T.
    fn deref(&self) -> &T {
        unsafe { &*(&self.ptr as *const *mut _ as *const T) }
    }
}

impl<'lib, T> Symbol<'lib, T> {
    /// Consumes the symbol and returns the raw pointer
    ///
    /// This method converts the symbol into a raw pointer, transferring
    /// ownership to the caller.
    ///
    /// # Returns
    /// A raw pointer to the symbol data
    pub fn into_raw(self) -> *const () {
        self.ptr
    }
}

// Safety: Symbol can be sent between threads if T can
unsafe impl<T: Send> Send for Symbol<'_, T> {}

// Safety: Symbol can be shared between threads if T can
unsafe impl<T: Sync> Sync for Symbol<'_, T> {}

/// A relocated ELF object
///
/// This structure represents an ELF file that has been loaded and relocated
/// in memory. It maintains references to its dependencies to prevent
/// premature deallocation.
#[derive(Debug, Clone)]
pub struct Relocated<'scope> {
    /// The core component containing the actual ELF data
    pub(crate) core: CoreComponent,

    /// Phantom data to maintain lifetime information
    pub(crate) _marker: PhantomData<&'scope ()>,
}

impl Relocated<'_> {
    /// Creates a Relocated instance from a CoreComponent
    ///
    /// # Safety
    /// The current ELF object has not yet been relocated, so it is dangerous
    /// to use this function to convert `CoreComponent` to `RelocateDylib`.
    /// Lifecycle information is lost.
    ///
    /// # Arguments
    /// * `core` - The CoreComponent to wrap
    ///
    /// # Returns
    /// A new Relocated instance
    #[inline]
    pub unsafe fn from_core_component(core: CoreComponent) -> Self {
        Relocated {
            core,
            _marker: PhantomData,
        }
    }

    /// Gets the core component reference of the ELF object
    ///
    /// # Safety
    /// Lifecycle information is lost, and the dependencies of the current
    /// ELF object can be prematurely deallocated, which can cause serious problems.
    ///
    /// # Returns
    /// A reference to the CoreComponent
    #[inline]
    pub unsafe fn core_component_ref(&self) -> &CoreComponent {
        &self.core
    }

    /// Creates a new Relocated instance without validation
    ///
    /// # Safety
    /// The caller needs to ensure that the parameters passed in come
    /// from a valid dynamic library.
    ///
    /// # Arguments
    /// * `name` - The name of the ELF file
    /// * `base` - The base address where the ELF is loaded
    /// * `dynamic` - The parsed dynamic section
    /// * `phdrs` - The program headers
    /// * `segments` - The loaded segments
    /// * `user_data` - User-defined data to associate with the ELF
    ///
    /// # Returns
    /// A new Relocated instance
    #[inline]
    pub unsafe fn new_uncheck(
        name: CString,
        base: usize,
        dynamic: ElfDynamic,
        phdrs: &'static [ElfPhdr],
        segments: ElfSegments,
        user_data: UserData,
    ) -> Self {
        Self {
            core: CoreComponent::from_raw(name, base, dynamic, phdrs, segments, user_data),
            _marker: PhantomData,
        }
    }

    /// Gets the symbol table
    ///
    /// # Returns
    /// A reference to the SymbolTable
    pub fn symtab(&self) -> &SymbolTable {
        unsafe { self.core.symtab().unwrap_unchecked() }
    }

    /// Gets a pointer to a function or static variable by symbol name
    ///
    /// The symbol is interpreted as-is; no mangling is done. This means
    /// that symbols like `x::y` are most likely invalid.
    ///
    /// # Safety
    /// Users of this API must specify the correct type of the function
    /// or variable loaded.
    ///
    /// # Examples
    /// ```no_run
    /// # use elf_loader::{object::ElfBinary, Symbol, mmap::MmapImpl, Loader};
    /// # let mut loader = Loader::<MmapImpl>::new();
    /// # let lib = loader
    /// #     .easy_load_dylib(ElfBinary::new("target/liba.so", &[]))
    /// #        .unwrap().easy_relocate([].iter(), &|_|{None}).unwrap();
    /// unsafe {
    ///     let awesome_function: Symbol<unsafe extern fn(f64) -> f64> =
    ///         lib.get("awesome_function").unwrap();
    ///     awesome_function(0.42);
    /// }
    /// ```
    ///
    /// A static variable may also be loaded and inspected:
    /// ```no_run
    /// # use elf_loader::{object::ElfBinary, Symbol, mmap::MmapImpl, Loader};
    /// # let mut loader = Loader::<MmapImpl>::new();
    /// # let lib = loader
    /// #     .easy_load_dylib(ElfBinary::new("target/liba.so", &[]))
    /// #        .unwrap().easy_relocate([].iter(), &|_|{None}).unwrap();
    /// unsafe {
    ///     let awesome_variable: Symbol<*mut f64> = lib.get("awesome_variable").unwrap();
    ///     **awesome_variable = 42.0;
    /// };
    /// ```
    ///
    /// # Arguments
    /// * `name` - The name of the symbol to look up
    ///
    /// # Returns
    /// * `Some(symbol)` - If the symbol is found
    /// * `None` - If the symbol is not found
    #[inline]
    pub unsafe fn get<'lib, T>(&'lib self, name: &str) -> Option<Symbol<'lib, T>> {
        let syminfo = SymbolInfo::from_str(name, None);
        let mut precompute = syminfo.precompute();
        self.symtab()
            .lookup_filter(&syminfo, &mut precompute)
            .map(|sym| Symbol {
                ptr: SymDef {
                    sym: Some(sym),
                    lib: self,
                }
                .convert() as _,
                pd: PhantomData,
            })
    }

    /// Load a versioned symbol from the ELF object
    ///
    /// # Safety
    /// Users of this API must specify the correct type of the function
    /// or variable loaded.
    ///
    /// # Examples
    /// ```no_run
    /// # use elf_loader::{object::ElfFile, Symbol, mmap::MmapImpl, Loader};
    /// # let mut loader = Loader::<MmapImpl>::new();
    /// # let lib = loader
    /// #     .easy_load_dylib(ElfFile::from_path("target/liba.so").unwrap())
    /// #        .unwrap().easy_relocate([].iter(), &|_|{None}).unwrap();;
    /// let symbol = unsafe { lib.get_version::<fn()>("function_name", "1.0").unwrap() };
    /// ```
    ///
    /// # Arguments
    /// * `name` - The name of the symbol to look up
    /// * `version` - The version of the symbol to look up
    ///
    /// # Returns
    /// * `Some(symbol)` - If the symbol is found
    /// * `None` - If the symbol is not found
    #[cfg(feature = "version")]
    #[inline]
    pub unsafe fn get_version<'lib, T>(
        &'lib self,
        name: &str,
        version: &str,
    ) -> Option<Symbol<'lib, T>> {
        let syminfo = SymbolInfo::from_str(name, Some(version));
        let mut precompute = syminfo.precompute();
        self.symtab()
            .lookup_filter(&syminfo, &mut precompute)
            .map(|sym| Symbol {
                ptr: SymDef {
                    sym: Some(sym),
                    lib: self,
                }
                .convert() as _,
                pd: PhantomData,
            })
    }
}

/// Internal representation of ELF program headers
#[derive(Clone)]
enum ElfPhdrs {
    /// Program headers mapped from memory
    Mmap(&'static [ElfPhdr]),

    /// Program headers stored in a vector
    Vec(Vec<ElfPhdr>),
}

/// Inner structure for CoreComponent
pub(crate) struct CoreComponentInner {
    /// Indicates whether the component has been initialized
    is_init: AtomicBool,

    /// File name of the ELF object
    name: CString,

    /// ELF symbols table
    pub(crate) symbols: Option<SymbolTable>,

    /// Dynamic section pointer
    dynamic: Option<NonNull<Dyn>>,

    /// PLT relocations
    #[allow(unused)]
    pub(crate) pltrel: Option<NonNull<ElfRelType>>,

    /// Program headers
    phdrs: ElfPhdrs,

    /// Finalization function
    fini: Option<fn()>,

    /// Finalization array of functions
    fini_array: Option<&'static [fn()]>,

    /// Custom finalization handler
    fini_handler: FnHandler,

    /// Names of needed libraries
    needed_libs: Box<[&'static str]>,

    /// User-defined data
    user_data: UserData,

    /// Lazy binding scope
    pub(crate) lazy_scope: Option<LazyScope<'static>>,

    /// Memory segments
    pub(crate) segments: ElfSegments,
}

impl Drop for CoreComponentInner {
    /// Executes finalization functions when the component is dropped
    fn drop(&mut self) {
        if self.is_init.load(Ordering::Relaxed) {
            (self.fini_handler)(self.fini, self.fini_array);
        }
    }
}

/// `CoreComponentRef` is a version of `CoreComponent` that holds a non-owning reference to the managed allocation.
#[derive(Clone)]
pub struct CoreComponentRef {
    /// Weak reference to the CoreComponentInner
    inner: Weak<CoreComponentInner>,
}

impl CoreComponentRef {
    /// Attempts to upgrade the Weak pointer to an Arc
    ///
    /// # Returns
    /// * `Some(CoreComponent)` - If the upgrade is successful
    /// * `None` - If the CoreComponent has been dropped
    pub fn upgrade(&self) -> Option<CoreComponent> {
        self.inner.upgrade().map(|inner| CoreComponent { inner })
    }
}

/// The core part of an ELF object
///
/// This structure represents the core data of an ELF object, including
/// its metadata, symbols, segments, and other essential information.
#[derive(Clone)]
pub struct CoreComponent {
    /// Shared reference to the inner component data
    pub(crate) inner: Arc<CoreComponentInner>,
}

// Safety: CoreComponentInner can be shared between threads
unsafe impl Sync for CoreComponentInner {}

// Safety: CoreComponentInner can be sent between threads
unsafe impl Send for CoreComponentInner {}

impl CoreComponent {
    /// Sets the lazy scope for this component
    ///
    /// # Arguments
    /// * `lazy_scope` - The lazy scope to set
    #[inline]
    pub(crate) fn set_lazy_scope(&self, lazy_scope: LazyScope) {
        // 因为在完成重定位前，只有unsafe的方法可以拿到CoreComponent的引用，所以这里认为是安全的
        unsafe {
            let ptr = &mut *(Arc::as_ptr(&self.inner) as *mut CoreComponentInner);
            // 在relocate接口处保证了lazy_scope的声明周期，因此这里直接转换
            ptr.lazy_scope = Some(core::mem::transmute::<LazyScope<'_>, LazyScope<'static>>(
                lazy_scope,
            ));
        };
    }

    /// Marks the component as initialized
    #[inline]
    pub(crate) fn set_init(&self) {
        self.inner.is_init.store(true, Ordering::Relaxed);
    }

    /// Creates a new Weak pointer to this allocation
    ///
    /// # Returns
    /// A CoreComponentRef that holds a weak reference to this component
    #[inline]
    pub fn downgrade(&self) -> CoreComponentRef {
        CoreComponentRef {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// Gets user data from the ELF object
    ///
    /// # Returns
    /// A reference to the user data
    #[inline]
    pub fn user_data(&self) -> &UserData {
        &self.inner.user_data
    }

    /// Gets the number of strong references to the ELF object
    ///
    /// # Returns
    /// The number of strong (Arc) references
    #[inline]
    pub fn strong_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    /// Gets the number of weak references to the ELF object
    ///
    /// # Returns
    /// The number of weak references
    #[inline]
    pub fn weak_count(&self) -> usize {
        Arc::weak_count(&self.inner)
    }

    /// Gets the name of the ELF object
    ///
    /// # Returns
    /// The name of the ELF object as a string slice
    #[inline]
    pub fn name(&self) -> &str {
        self.inner.name.to_str().unwrap()
    }

    /// Gets the C-style name of the ELF object
    ///
    /// # Returns
    /// The name of the ELF object as a C string
    #[inline]
    pub fn cname(&self) -> &CStr {
        &self.inner.name
    }

    /// Gets the short name of the ELF object
    ///
    /// This method returns just the filename portion without any path components.
    ///
    /// # Returns
    /// The short name (filename only) of the ELF object
    #[inline]
    pub fn shortname(&self) -> &str {
        self.name().split('/').next_back().unwrap()
    }

    /// Gets the base address of the ELF object
    ///
    /// # Returns
    /// The base address where the ELF object is loaded in memory
    #[inline]
    pub fn base(&self) -> usize {
        self.inner.segments.base()
    }

    /// Gets the memory length of the ELF object map
    ///
    /// # Returns
    /// The total length of memory occupied by the ELF object
    #[inline]
    pub fn map_len(&self) -> usize {
        self.inner.segments.len()
    }

    /// Gets the program headers of the ELF object
    ///
    /// # Returns
    /// A slice of the program headers
    #[inline]
    pub fn phdrs(&self) -> &[ElfPhdr] {
        match &self.inner.phdrs {
            ElfPhdrs::Mmap(phdrs) => &phdrs,
            ElfPhdrs::Vec(phdrs) => &phdrs,
        }
    }

    /// Gets the address of the dynamic section
    ///
    /// # Returns
    /// An optional NonNull pointer to the dynamic section
    #[inline]
    pub fn dynamic(&self) -> Option<NonNull<Dyn>> {
        self.inner.dynamic
    }

    /// Gets the needed libs' name of the ELF object
    ///
    /// # Returns
    /// A slice of the names of libraries this ELF object depends on
    #[inline]
    pub fn needed_libs(&self) -> &[&str] {
        &self.inner.needed_libs
    }

    /// Gets the symbol table
    ///
    /// # Returns
    /// An optional reference to the symbol table
    #[inline]
    pub fn symtab(&self) -> Option<&SymbolTable> {
        self.inner.symbols.as_ref()
    }

    /// Gets the segments
    ///
    /// # Returns
    /// A reference to the ELF segments
    #[inline]
    pub(crate) fn segments(&self) -> &ElfSegments {
        &self.inner.segments
    }

    /// Creates a CoreComponent from raw data
    ///
    /// # Arguments
    /// * `name` - The name of the ELF file
    /// * `base` - The base address where the ELF is loaded
    /// * `dynamic` - The parsed dynamic section
    /// * `phdrs` - The program headers
    /// * `segments` - The loaded segments
    /// * `user_data` - User-defined data to associate with the ELF
    ///
    /// # Returns
    /// A new CoreComponent instance
    fn from_raw(
        name: CString,
        base: usize,
        dynamic: ElfDynamic,
        phdrs: &'static [ElfPhdr],
        mut segments: ElfSegments,
        user_data: UserData,
    ) -> Self {
        segments.offset = (segments.memory.as_ptr() as usize).wrapping_sub(base);
        Self {
            inner: Arc::new(CoreComponentInner {
                name,
                is_init: AtomicBool::new(true),
                symbols: Some(SymbolTable::from_dynamic(&dynamic)),
                pltrel: None,
                dynamic: NonNull::new(dynamic.dyn_ptr as _),
                phdrs: ElfPhdrs::Mmap(phdrs),
                segments,
                fini: None,
                fini_array: None,
                fini_handler: Arc::new(|_, _| {}),
                needed_libs: Box::new([]),
                user_data,
                lazy_scope: None,
            }),
        }
    }
}

impl Debug for CoreComponent {
    /// Formats the CoreComponent for debugging purposes
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dylib")
            .field("name", &self.inner.name)
            .finish()
    }
}

impl<M: Mmap> Loader<M> {
    /// Load an ELF file into memory
    ///
    /// This is a convenience method that calls [load] with `lazy_bind` set to `None`.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load
    ///
    /// # Returns
    /// * `Ok(Elf)` - The loaded ELF file
    /// * `Err(Error)` - If loading fails
    pub fn easy_load(&mut self, object: impl ElfObject) -> Result<Elf> {
        self.load(object, None)
    }

    /// Load an ELF file into memory
    ///
    /// # Note
    /// * When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load
    /// * `lazy_bind` - Optional override for lazy binding behavior
    ///
    /// # Returns
    /// * `Ok(Elf)` - The loaded ELF file
    /// * `Err(Error)` - If loading fails
    pub fn load(&mut self, mut object: impl ElfObject, lazy_bind: Option<bool>) -> Result<Elf> {
        let ehdr = self.buf.prepare_ehdr(&mut object)?;
        let is_dylib = ehdr.is_dylib();
        if is_dylib {
            Ok(Elf::Dylib(self.load_dylib(object, lazy_bind)?))
        } else {
            Ok(Elf::Exec(self.load_exec(object, lazy_bind)?))
        }
    }


}
