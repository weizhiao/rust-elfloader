//! ELF file format handling
//!
//! This module provides the core data structures and functionality for working
//! with ELF files in various stages of processing: from raw ELF files to
//! relocated and loaded libraries or executables.

use self::relocatable::ElfRelocatable;
use crate::{
    Hook, Loader, Result,
    arch::{Dyn, ElfPhdr, ElfRelType},
    dynamic::ElfDynamic,
    format::relocated::{ElfDylib, ElfExec, RelocatedDylib, RelocatedExec},
    loader::FnHandler,
    mmap::Mmap,
    object::ElfObject,
    relocation::{Relocatable, RelocationHandler, SymDef, SymbolLookup},
    segment::ElfSegments,
    symbol::{SymbolInfo, SymbolTable},
};
use alloc::{boxed::Box, ffi::CString, vec::Vec};
use core::{
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

pub(crate) mod relocatable;
pub(crate) mod relocated;

impl<D> Deref for Relocated<D> {
    type Target = CoreComponent<D>;

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
pub enum Elf<D>
where
    D: 'static,
{
    /// A dynamic library (shared object)
    Dylib(ElfDylib<D>),

    /// An executable file
    Exec(ElfExec<D>),

    /// A relocatable file (object file)
    Relocatable(ElfRelocatable),
}

/// An ELF file that has been relocated
///
/// This enum represents an ELF file that has been loaded and relocated.
/// It maintains lifetime information to prevent premature deallocation
/// of dependencies.
#[derive(Debug, Clone)]
pub enum RelocatedElf<D> {
    /// A relocated dynamic library
    Dylib(RelocatedDylib<D>),

    /// A relocated executable
    Exec(RelocatedExec<D>),

    /// A relocated relocatable file (always uses () for user_data)
    Relocatable(Relocated<()>),
}

impl<D> RelocatedElf<D> {
    /// Converts this RelocatedElf into a RelocatedDylib if it is one
    ///
    /// # Returns
    /// * `Some(dylib)` - If this is a Dylib variant
    /// * `None` - If this is an Exec variant
    #[inline]
    pub fn into_dylib(self) -> Option<RelocatedDylib<D>> {
        match self {
            RelocatedElf::Dylib(dylib) => Some(dylib),
            _ => None,
        }
    }

    /// Converts this RelocatedElf into a RelocatedExec if it is one
    ///
    /// # Returns
    /// * `Some(exec)` - If this is an Exec variant
    /// * `None` - If this is a Dylib variant
    #[inline]
    pub fn into_exec(self) -> Option<RelocatedExec<D>> {
        match self {
            RelocatedElf::Exec(exec) => Some(exec),
            _ => None,
        }
    }

    /// Gets a reference to the RelocatedDylib if this is one
    ///
    /// # Returns
    /// * `Some(dylib)` - If this is a Dylib variant
    /// * `None` - If this is an Exec variant
    #[inline]
    pub fn as_dylib(&self) -> Option<&RelocatedDylib<D>> {
        match self {
            RelocatedElf::Dylib(dylib) => Some(dylib),
            _ => None,
        }
    }
}

impl<D> Deref for Elf<D> {
    type Target = CoreComponent<D>;

    /// Dereferences to the underlying CoreComponent
    ///
    /// This allows direct access to common fields shared by all ELF file types.
    ///
    /// # Panics
    /// Panics if called on a Relocatable variant, as relocatable files always use `CoreComponent<()>`.
    fn deref(&self) -> &Self::Target {
        match self {
            Elf::Dylib(elf_dylib) => elf_dylib.core_ref(),
            Elf::Exec(elf_exec) => elf_exec.core_ref(),
            Elf::Relocatable(_) => panic!("Deref not supported for Relocatable variant"),
        }
    }
}

impl<D: 'static> Relocatable<D> for Elf<D> {
    type Output = RelocatedElf<D>;

    fn relocate<S, LazyS, PreH, PostH>(
        self,
        scope: &[Relocated<D>],
        pre_find: &S,
        pre_handler: PreH,
        post_handler: PostH,
        lazy: Option<bool>,
        lazy_scope: Option<LazyS>,
        use_scope_as_lazy: bool,
    ) -> Result<Self::Output>
    where
        D: 'static,
        S: SymbolLookup + ?Sized,
        LazyS: SymbolLookup + Send + Sync + 'static,
        PreH: RelocationHandler,
        PostH: RelocationHandler,
    {
        match self {
            Elf::Dylib(dylib) => {
                let relocated = Relocatable::relocate(
                    dylib,
                    scope,
                    pre_find,
                    pre_handler,
                    post_handler,
                    lazy,
                    lazy_scope,
                    use_scope_as_lazy,
                )?;
                Ok(RelocatedElf::Dylib(relocated))
            }
            Elf::Exec(exec) => {
                let relocated = Relocatable::relocate(
                    exec,
                    scope,
                    pre_find,
                    pre_handler,
                    post_handler,
                    lazy,
                    lazy_scope,
                    use_scope_as_lazy,
                )?;
                Ok(RelocatedElf::Exec(relocated))
            }
            Elf::Relocatable(relocatable) => {
                let relocated = Relocatable::relocate(
                    relocatable,
                    &[],
                    pre_find,
                    pre_handler,
                    post_handler,
                    lazy,
                    None::<()>, // ElfRelocatable always uses LazyScope<(), ()>, so pass None
                    use_scope_as_lazy,
                )?;
                Ok(RelocatedElf::Relocatable(relocated))
            }
        }
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
#[derive(Debug)]
pub struct Relocated<D> {
    /// The core component containing the actual ELF data
    pub(crate) core: CoreComponent<D>,
    /// The dependencies of the ELF object
    pub(crate) deps: Arc<[Relocated<D>]>,
}

impl<D> Clone for Relocated<D> {
    fn clone(&self) -> Self {
        Relocated {
            core: self.core.clone(),
            deps: Arc::clone(&self.deps),
        }
    }
}

impl<D> Relocated<D> {
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
    pub unsafe fn from_core(core: CoreComponent<D>) -> Self {
        Relocated {
            core,
            deps: Arc::from([]),
        }
    }

    /// Gets the dependencies of the ELF object (short name `deps`)
    pub fn deps(&self) -> &[Relocated<D>] {
        &self.deps
    }

    /// Creates a Relocated instance from a CoreComponent and dependencies
    ///
    /// # Safety
    /// The current ELF object has not yet been relocated, so it is dangerous
    /// to use this function to convert `CoreComponent` to `RelocateDylib`.
    /// Lifecycle information is lost.
    ///
    /// # Arguments
    /// * `core` - The CoreComponent to wrap
    /// * `deps` - The dependencies of the ELF object
    ///
    /// # Returns
    /// A new Relocated instance
    #[inline]
    pub unsafe fn from_core_deps(core: CoreComponent<D>, deps: Vec<Relocated<D>>) -> Self {
        Relocated {
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
    /// A reference to the CoreComponent
    #[inline]
    pub unsafe fn core_ref(&self) -> &CoreComponent<D> {
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
    pub unsafe fn new_unchecked(
        name: CString,
        base: usize,
        dynamic: ElfDynamic,
        phdrs: &'static [ElfPhdr],
        segments: ElfSegments,
        user_data: D,
    ) -> Self {
        Self {
            core: CoreComponent::from_raw(name, base, dynamic, phdrs, segments, user_data),
            deps: Arc::from([]),
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
    /// # use elf_loader::{object::ElfBinary, Symbol, Loader, Relocatable};
    /// # let mut loader = Loader::new();
    /// # let lib = loader
    /// #     .load_dylib(ElfBinary::new("target/liba.so", &[]))
    /// #        .unwrap().relocator().symbols(&| _: &str| None).scope([].iter()).relocate().unwrap();
    /// unsafe {
    ///     let awesome_function: Symbol<unsafe extern "C" fn(f64) -> f64> =
    ///         lib.get("awesome_function").unwrap();
    ///     awesome_function(0.42);
    /// }
    /// ```
    ///
    /// A static variable may also be loaded and inspected:
    /// ```no_run
    /// # use elf_loader::{object::ElfBinary, Symbol, Loader, Relocatable};
    /// # let mut loader = Loader::new();
    /// # let lib = loader
    /// #     .load_dylib(ElfBinary::new("target/liba.so", &[]))
    /// #        .unwrap().relocator().symbols(&| _: &str| None).scope([].iter()).relocate().unwrap();
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
    /// # use elf_loader::{object::ElfFile, Symbol, mmap::DefaultMmap, Loader};
    /// # let mut loader = Loader::new();
    /// # let lib = loader
    /// #     .load_dylib(ElfFile::from_path("target/liba.so").unwrap())
    /// #        .unwrap().relocate([].iter(), &| _: &str| None).unwrap();;
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
pub(crate) enum ElfPhdrs {
    /// Program headers mapped from memory
    Mmap(&'static [ElfPhdr]),

    /// Program headers stored in a vector
    Vec(Vec<ElfPhdr>),
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

impl ElfPhdrs {
    fn as_slice(&self) -> &[ElfPhdr] {
        match self {
            ElfPhdrs::Mmap(phdrs) => phdrs,
            ElfPhdrs::Vec(phdrs) => phdrs,
        }
    }
}

pub(crate) struct DynamicComponent {
    pub(crate) dynamic: NonNull<Dyn>,
    #[allow(dead_code)]
    pub(crate) pltrel: Option<NonNull<ElfRelType>>,
    pub(crate) phdrs: ElfPhdrs,
    pub(crate) needed_libs: Box<[&'static str]>,
    /// Lazy binding scope for symbol resolution during lazy binding
    /// Stored as trait object for type erasure of different SymbolLookup implementations
    pub(crate) lazy_scope: Option<Arc<dyn SymbolLookup>>,
}

/// Inner structure for CoreComponent
pub(crate) struct CoreComponentInner<D = ()> {
    /// Indicates whether the component has been initialized
    is_init: AtomicBool,

    /// File name of the ELF object
    name: CString,

    /// ELF symbols table
    pub(crate) symbols: Option<SymbolTable>,

    /// Dynamic component
    pub(crate) dynamic_info: Option<DynamicComponent>,

    /// Finalization function
    fini: Option<fn()>,

    /// Finalization array of functions
    fini_array: Option<&'static [fn()]>,

    /// Custom finalization handler
    fini_handler: FnHandler,

    /// User-defined data
    user_data: D,

    /// Memory segments
    pub(crate) segments: ElfSegments,

    /// Indicates the type of the ELF file
    pub(crate) elf_type: ElfType,
}

impl<D> Drop for CoreComponentInner<D> {
    /// Executes finalization functions when the component is dropped
    fn drop(&mut self) {
        if self.is_init.load(Ordering::Relaxed) {
            (self.fini_handler)(self.fini, self.fini_array);
        }
    }
}

/// `CoreComponentRef` is a version of `CoreComponent` that holds a non-owning reference to the managed allocation.
#[derive(Clone)]
pub struct CoreComponentRef<D = ()> {
    /// Weak reference to the CoreComponentInner
    inner: Weak<CoreComponentInner<D>>,
}

impl<D> CoreComponentRef<D> {
    /// Attempts to upgrade the Weak pointer to an Arc
    ///
    /// # Returns
    /// * `Some(CoreComponent)` - If the upgrade is successful
    /// * `None` - If the CoreComponent has been dropped
    pub fn upgrade(&self) -> Option<CoreComponent<D>> {
        self.inner.upgrade().map(|inner| CoreComponent { inner })
    }
}

/// The core part of an ELF object
///
/// This structure represents the core data of an ELF object, including
/// its metadata, symbols, segments, and other essential information.
pub struct CoreComponent<D = ()> {
    /// Shared reference to the inner component data
    pub(crate) inner: Arc<CoreComponentInner<D>>,
}

impl<D> Clone for CoreComponent<D> {
    fn clone(&self) -> Self {
        CoreComponent {
            inner: Arc::clone(&self.inner),
        }
    }
}

// Safety: CoreComponentInner can be shared between threads
unsafe impl<D> Sync for CoreComponentInner<D> {}
// Safety: CoreComponentInner can be sent between threads
unsafe impl<D> Send for CoreComponentInner<D> {}

impl<D> CoreComponent<D> {
    /// Sets the lazy scope for this component
    ///
    /// # Arguments
    /// * `lazy_scope` - The lazy scope to set
    #[inline]
    pub(crate) fn set_lazy_scope<LazyS>(&self, lazy_scope: LazyS)
    where
        D: 'static,
        LazyS: SymbolLookup + Send + Sync + 'static,
    {
        // 因为在完成重定位前，只有unsafe的方法可以拿到CoreComponent的引用，所以这里认为是安全的
        // LazyScope 会被长期存储用于延迟绑定符号查询，因此需要 Send + Sync + 'static 约束
        // 注意：D 的生命周期由 CoreComponentInner<D> 保证
        unsafe {
            let ptr = &mut *(Arc::as_ptr(&self.inner) as *mut CoreComponentInner<D>);
            // 在relocate接口处保证了lazy_scope的声明周期，因此这里直接转换
            ptr.dynamic_info.as_mut().unwrap().lazy_scope = Some(Arc::new(lazy_scope));
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
    pub fn downgrade(&self) -> CoreComponentRef<D> {
        CoreComponentRef {
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
        self.inner
            .dynamic_info
            .as_ref()
            .map(|d| d.phdrs.as_slice())
            .unwrap_or(&[])
    }

    /// Gets the address of the dynamic section
    ///
    /// # Returns
    /// An optional NonNull pointer to the dynamic section
    #[inline]
    pub fn dynamic(&self) -> Option<NonNull<Dyn>> {
        self.inner.dynamic_info.as_ref().map(|d| d.dynamic)
    }

    /// Gets the needed libs' name of the ELF object
    ///
    /// # Returns
    /// A slice of the names of libraries this ELF object depends on
    #[inline]
    pub fn needed_libs(&self) -> &[&str] {
        self.inner
            .dynamic_info
            .as_ref()
            .map(|d| &*d.needed_libs)
            .unwrap_or(&[])
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
        user_data: D,
    ) -> Self {
        segments.offset = (segments.memory.as_ptr() as usize).wrapping_sub(base);
        Self {
            inner: Arc::new(CoreComponentInner {
                name,
                is_init: AtomicBool::new(true),
                symbols: Some(SymbolTable::from_dynamic(&dynamic)),
                dynamic_info: Some(DynamicComponent {
                    dynamic: NonNull::new(dynamic.dyn_ptr as _).unwrap(),
                    pltrel: None,
                    phdrs: ElfPhdrs::Mmap(phdrs),
                    needed_libs: Box::new([]),
                    lazy_scope: None,
                }),
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

impl<D> Debug for CoreComponent<D> {
    /// Formats the CoreComponent for debugging purposes
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dylib")
            .field("name", &self.inner.name)
            .finish()
    }
}

impl<M: Mmap, H: Hook<D>, D: Default> Loader<M, H, D> {
    /// Load an ELF file into memory
    ///
    /// # Arguments
    /// * `object` - The ELF object to load
    ///
    /// # Returns
    /// * `Ok(Elf)` - The loaded ELF file
    /// * `Err(Error)` - If loading fails
    pub fn load(&mut self, mut object: impl ElfObject) -> Result<Elf<D>> {
        let ehdr = self.buf.prepare_ehdr(&mut object)?;
        let is_dylib = ehdr.is_dylib();
        if is_dylib {
            Ok(Elf::Dylib(self.load_dylib(object)?))
        } else if ehdr.e_type == elf::abi::ET_REL {
            // Relocatable files don't use user_data, so we call load_rel directly
            Ok(Elf::Relocatable(self.load_rel(ehdr, object)?))
        } else {
            Ok(Elf::Exec(self.load_exec(object)?))
        }
    }
}
