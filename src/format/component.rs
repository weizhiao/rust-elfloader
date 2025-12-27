//! ELF file format handling
//!
//! This module provides the core data structures and functionality for working
//! with ELF files in various stages of processing: from raw ELF files to
//! relocated and loaded libraries or executables.

use crate::{
    arch::{Dyn, ElfPhdr},
    dynamic::ElfDynamic,
    format::image::{DynamicInfo, ElfPhdrs},
    loader::FnHandler,
    relocation::SymDef,
    segment::ElfSegments,
    symbol::{SymbolInfo, SymbolTable},
};
use alloc::{string::String, vec::Vec};
use core::{
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

impl<D> Deref for LoadedModule<D> {
    type Target = ElfModule<D>;

    /// Dereferences to the underlying ElfModule
    ///
    /// This allows direct access to the ElfModule fields through the Relocated wrapper.
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

/// A typed symbol retrieved from a loaded ELF module.
///
/// `Symbol` provides safe access to a function or variable within a loaded library.
/// It carries a lifetime marker `'lib` to ensure that the symbol cannot outlive
/// the library it was loaded from, preventing use-after-free errors.
#[derive(Debug, Clone)]
pub struct Symbol<'lib, T: 'lib> {
    /// Raw pointer to the symbol's memory location.
    ptr: *mut (),

    /// Phantom data to bind the symbol's lifetime to the source library.
    pd: PhantomData<&'lib T>,
}

impl<'lib, T> Deref for Symbol<'lib, T> {
    type Target = T;

    /// Accesses the underlying symbol as a reference to type `T`.
    ///
    /// This allows calling functions or accessing variables directly.
    ///
    /// # Returns
    /// A reference to the symbol of type `T`.
    fn deref(&self) -> &T {
        unsafe { &*(&self.ptr as *const *mut _ as *const T) }
    }
}

impl<'lib, T> Symbol<'lib, T> {
    /// Consumes the `Symbol` and returns its raw memory address.
    ///
    /// # Returns
    /// A raw pointer to the symbol data.
    pub fn into_raw(self) -> *const () {
        self.ptr
    }
}

// Safety: Symbol can be sent between threads if T can
unsafe impl<T: Send> Send for Symbol<'_, T> {}

// Safety: Symbol can be shared between threads if T can
unsafe impl<T: Sync> Sync for Symbol<'_, T> {}

/// A fully loaded and relocated ELF module.
///
/// This structure represents an ELF object (executable, shared library, or relocatable object)
/// that has been mapped into memory and had its relocations performed.
///
/// It maintains an `Arc` reference to its dependencies to ensure that required
/// libraries remain in memory as long as this module is alive.
#[derive(Debug)]
pub struct LoadedModule<D> {
    /// The core ELF module data and metadata.
    pub(crate) core: ElfModule<D>,
    /// Shared references to the libraries this module depends on.
    pub(crate) deps: Arc<[LoadedModule<D>]>,
}

impl<D> Clone for LoadedModule<D> {
    /// Clones the [`LoadedModule`], incrementing the reference count of its core and dependencies.
    fn clone(&self) -> Self {
        LoadedModule {
            core: self.core.clone(),
            deps: Arc::clone(&self.deps),
        }
    }
}

impl<D> LoadedModule<D> {
    /// Wraps an [`ElfModule`] into a [`LoadedModule`] with no dependencies.
    ///
    /// # Safety
    /// The caller must ensure the ELF object has been properly relocated.
    ///
    /// # Arguments
    /// * `core` - The [`ElfModule`] to wrap.
    #[inline]
    pub unsafe fn from_core(core: ElfModule<D>) -> Self {
        LoadedModule {
            core,
            deps: Arc::from([]),
        }
    }

    /// Returns a slice of the libraries this module depends on.
    pub fn deps(&self) -> &[LoadedModule<D>] {
        &self.deps
    }

    /// Creates a [`LoadedModule`] from an [`ElfModule`] and its explicit dependencies.
    ///
    /// # Safety
    /// The caller must ensure the ELF object has been properly relocated.
    ///
    /// # Arguments
    /// * `core` - The [`ElfModule`] to wrap.
    /// * `deps` - A vector of dependencies.
    #[inline]
    pub unsafe fn from_core_deps(core: ElfModule<D>, deps: Vec<LoadedModule<D>>) -> Self {
        LoadedModule {
            core,
            deps: Arc::from(deps),
        }
    }

    /// Gets the core component reference of the ELF object
    ///
    /// # Safety
    /// Lifecycle information is lost, and the dependencies of the current
    /// ELF object can be prematurely deallocated, which can cause serious problems.
    ///
    /// # Returns
    /// A reference to the ElfModule
    #[inline]
    pub unsafe fn core_ref(&self) -> &ElfModule<D> {
        &self.core
    }

    /// Creates a new LoadedModule instance without validation
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
    /// A new LoadedModule instance
    #[inline]
    pub unsafe fn new_unchecked(
        name: String,
        base: usize,
        dynamic: ElfDynamic,
        phdrs: &'static [ElfPhdr],
        segments: ElfSegments,
        user_data: D,
    ) -> Self {
        Self {
            core: ElfModule::from_raw(name, base, dynamic, phdrs, segments, user_data),
            deps: Arc::from([]),
        }
    }

    /// Gets the symbol table
    ///
    /// # Returns
    /// A reference to the SymbolTable
    pub fn symtab(&self) -> &SymbolTable {
        &self.core.symtab()
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
    /// # use elf_loader::{ElfBinary, Symbol, Loader};
    /// # let mut loader = Loader::new();
    /// # let lib = loader
    /// #     .load_dylib(ElfBinary::new("target/liba.so", &[]))
    /// #        .unwrap().relocator().relocate().unwrap();
    /// unsafe {
    ///     let awesome_function = lib.get::<unsafe extern "C" fn(f64) -> f64>("awesome_function").unwrap();
    ///     awesome_function(0.42);
    /// }
    /// ```
    ///
    /// A static variable may also be loaded and inspected:
    /// ```no_run
    /// # use elf_loader::{ElfBinary, Symbol, Loader};
    /// # let mut loader = Loader::new();
    /// # let lib = loader
    /// #     .load_dylib(ElfBinary::new("target/liba.so", &[]))
    /// #        .unwrap().relocator().relocate().unwrap();
    /// unsafe {
    ///     let awesome_variable = lib.get::<*mut f64>("awesome_variable").unwrap();
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
    /// # use elf_loader::{ElfFile, Symbol, mmap::DefaultMmap, Loader};
    /// # let mut loader = Loader::new();
    /// # let lib = loader
    /// #     .load_dylib(ElfFile::from_path("target/liba.so").unwrap())
    /// #        .unwrap().relocator().relocate().unwrap();;
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

/// The type of the ELF file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfType {
    /// A dynamic library (shared object)
    Dylib,
    /// An executable file
    Exec,
    /// A relocatable file (object file)
    Relocatable,
}

impl ElfType {
    /// Returns true if the ELF file is a dynamic library
    #[inline]
    pub fn is_dylib(&self) -> bool {
        matches!(self, ElfType::Dylib)
    }
}

/// Inner structure for ElfModule
pub(crate) struct ModuleInner<D = ()> {
    /// Indicates whether the component has been initialized
    pub(crate) is_init: AtomicBool,

    /// File short name of the ELF object
    pub(crate) name: String,

    /// ELF symbols table
    pub(crate) symtab: SymbolTable,

    /// Finalization function
    pub(crate) fini: Option<fn()>,

    /// Finalization array of functions
    pub(crate) fini_array: Option<&'static [fn()]>,

    /// Custom finalization handler
    pub(crate) fini_handler: FnHandler,

    /// User-defined data
    pub(crate) user_data: D,

    /// Dynamic information
    pub(crate) dynamic_info: Option<Arc<DynamicInfo>>,

    /// Memory segments
    pub(crate) segments: ElfSegments,

    /// Indicates the type of the ELF file
    pub(crate) elf_type: ElfType,
}

impl<D> Drop for ModuleInner<D> {
    /// Executes finalization functions when the component is dropped
    fn drop(&mut self) {
        if self.is_init.load(Ordering::Relaxed) {
            (self.fini_handler)(self.fini, self.fini_array);
        }
    }
}

/// A non-owning reference to a [`ElfModule`].
///
/// `ElfModuleRef` holds a weak reference to the managed allocation of a
/// [`ElfModule`]. It can be used to avoid circular dependencies or to
/// check if the component is still alive.
#[derive(Clone)]
pub struct ElfModuleRef<D = ()> {
    /// Weak reference to the [`ModuleInner`].
    inner: Weak<ModuleInner<D>>,
}

impl<D> ElfModuleRef<D> {
    /// Attempts to upgrade the weak pointer to an [`ElfModule`].
    ///
    /// # Returns
    /// * `Some(ElfModule)` - If the component is still alive and the upgrade is successful.
    /// * `None` - If the [`ElfModule`] has been dropped.
    pub fn upgrade(&self) -> Option<ElfModule<D>> {
        self.inner.upgrade().map(|inner| ElfModule { inner })
    }
}

/// The core part of an ELF object.
///
/// This structure represents the core data of an ELF object, including
/// its metadata, symbols, segments, and other essential information.
/// It uses an [`Arc`] internally to manage the lifetime of the underlying data
/// and enable shared ownership.
pub struct ElfModule<D = ()> {
    /// Shared reference to the inner component data.
    pub(crate) inner: Arc<ModuleInner<D>>,
}

impl<D> Clone for ElfModule<D> {
    /// Clones the [`ElfModule`], incrementing the internal reference count.
    fn clone(&self) -> Self {
        ElfModule {
            inner: Arc::clone(&self.inner),
        }
    }
}

// Safety: ModuleInner can be shared between threads
unsafe impl<D> Sync for ModuleInner<D> {}
// Safety: ModuleInner can be sent between threads
unsafe impl<D> Send for ModuleInner<D> {}

impl<D> ElfModule<D> {
    /// Marks the component as initialized
    #[inline]
    pub(crate) fn set_init(&self) {
        self.inner.is_init.store(true, Ordering::Relaxed);
    }

    /// Creates a new Weak pointer to this allocation
    ///
    /// # Returns
    /// An ElfModuleRef that holds a weak reference to this component
    #[inline]
    pub fn downgrade(&self) -> ElfModuleRef<D> {
        ElfModuleRef {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// Gets user data from the ELF object
    ///
    /// # Returns
    /// A reference to the user data
    #[inline]
    pub fn user_data(&self) -> &D {
        &self.inner.user_data
    }

    pub fn phdrs(&self) -> Option<&[ElfPhdr]> {
        self.inner
            .dynamic_info
            .as_ref()
            .map(|info| info.phdrs.as_slice())
    }

    #[inline]
    pub fn user_data_mut(&mut self) -> Option<&mut D> {
        Arc::get_mut(&mut self.inner).map(|inner| &mut inner.user_data)
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

    /// Gets the type of the ELF object
    #[inline]
    pub fn elf_type(&self) -> ElfType {
        self.inner.elf_type
    }

    /// Gets the name of the ELF object
    ///
    /// # Returns
    /// The name of the ELF object as a string slice
    #[inline]
    pub fn name(&self) -> &str {
        &self.inner.name
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
    pub fn mapped_len(&self) -> usize {
        self.inner.segments.len()
    }

    /// Gets the symbol table
    ///
    /// # Returns
    /// An optional reference to the symbol table
    #[inline]
    pub fn symtab(&self) -> &SymbolTable {
        &self.inner.symtab
    }

    /// Gets a pointer to the dynamic section
    ///
    /// # Returns
    /// * `Some(ptr)` - A pointer to the dynamic section if it exists
    /// * `None` - If the dynamic section does not exist
    #[inline]
    pub fn dynamic_ptr(&self) -> Option<NonNull<Dyn>> {
        self.inner
            .dynamic_info
            .as_ref()
            .map(|info| info.dynamic_ptr)
    }

    /// Gets the segments
    ///
    /// # Returns
    /// A reference to the ELF segments
    #[inline]
    pub(crate) fn segments(&self) -> &ElfSegments {
        &self.inner.segments
    }

    /// Creates an ElfModule from raw data
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
    /// A new ElfModule instance
    fn from_raw(
        name: String,
        base: usize,
        dynamic: ElfDynamic,
        phdrs: &'static [ElfPhdr],
        mut segments: ElfSegments,
        user_data: D,
    ) -> Self {
        segments.offset = (segments.memory.as_ptr() as usize).wrapping_sub(base);
        Self {
            inner: Arc::new(ModuleInner {
                name,
                is_init: AtomicBool::new(true),
                symtab: SymbolTable::from_dynamic(&dynamic),
                dynamic_info: Some(Arc::new(DynamicInfo {
                    dynamic_ptr: NonNull::new(dynamic.dyn_ptr as _).unwrap(),
                    pltrel: None,
                    phdrs: ElfPhdrs::Mmap(phdrs),
                    lazy_scope: None,
                })),
                segments,
                fini: None,
                fini_array: None,
                fini_handler: Arc::new(|_, _| {}),
                user_data,
                elf_type: ElfType::Dylib,
            }),
        }
    }
}

impl<D> Debug for ElfModule<D> {
    /// Formats the ElfModule for debugging purposes
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dylib")
            .field("name", &self.inner.name)
            .finish()
    }
}
