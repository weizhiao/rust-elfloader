//! Relocated ELF file handling
//!
//! This module provides functionality for working with ELF files that have
//! been loaded and relocated in memory. It includes support for both dynamic
//! libraries (shared objects) and executables.

use crate::{
    CoreComponent, Result, UserData,
    arch::{Dyn, ElfPhdr},
    dynamic::ElfDynamic,
    ehdr::ElfHeader,
    format::{CoreComponentInner, ElfPhdrs},
    loader::{FnHandler, Hook},
    mmap::Mmap,
    parse_phdr_error,
    relocation::dynamic_link::DynamicRelocation,
    segment::{ELFRelro, ElfSegments},
    symbol::SymbolTable,
};
use alloc::{boxed::Box, ffi::CString, format, vec::Vec};
use core::{
    cell::Cell,
    ffi::{CStr, c_char},
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::{NonNull, null},
    sync::atomic::AtomicBool,
};
use delegate::delegate;
use elf::abi::PT_LOAD;
use elf::abi::{PT_DYNAMIC, PT_GNU_RELRO, PT_INTERP, PT_PHDR};

pub use dylib::{ElfDylib, RelocatedDylib};
pub use exec::{ElfExec, RelocatedExec};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

pub(crate) mod dylib;
pub(crate) mod exec;

/// Extra data associated with ELF objects during relocation
///
/// This structure holds additional data that is needed during the relocation
/// process but is not part of the core ELF object structure.
struct ElfExtraData {
    /// Indicates whether lazy binding is enabled for this object
    lazy: bool,

    /// Pointer to the Global Offset Table (.got.plt section)
    got: Option<NonNull<usize>>,

    /// Dynamic relocation information (rela.dyn and rela.plt)
    relocation: DynamicRelocation,

    /// GNU_RELRO segment information for memory protection
    relro: Option<ELFRelro>,

    /// Initialization function to be called after relocation
    init: Box<dyn Fn()>,

    /// DT_RPATH value from the dynamic section
    rpath: Option<&'static str>,

    /// DT_RUNPATH value from the dynamic section
    runpath: Option<&'static str>,
}

/// Data structure used during lazy parsing of ELF objects
///
/// This structure holds the data needed to initialize an ELF object
/// during the relocation process.
struct LazyData {
    /// Core component containing the basic ELF object information
    core: CoreComponent,

    /// Extra data needed for relocation
    extra: ElfExtraData,
}

/// State of an ELF object during the loading/relocation process
///
/// This enum represents the different states an ELF object can be in
/// during the loading and relocation process.
enum State {
    /// Initial empty state
    Empty,

    /// Uninitialized state with all necessary data to perform initialization
    Uninit {
        /// Indicates whether this is a dynamic library
        is_dylib: bool,

        /// Program headers
        phdrs: ElfPhdrs,

        /// Initialization function handler
        init_handler: FnHandler,

        /// Finalization function handler
        fini_handler: FnHandler,

        /// Name of the ELF file
        name: CString,

        /// Pointer to the dynamic section
        dynamic_ptr: Option<NonNull<Dyn>>,

        /// Memory segments
        segments: ElfSegments,

        /// GNU_RELRO segment information
        relro: Option<ELFRelro>,

        /// User-defined data
        user_data: UserData,

        /// Lazy binding override setting
        lazy_bind: Option<bool>,
    },

    /// Initialized state with all data ready for relocation
    Init(LazyData),
}

impl State {
    /// Initialize the state by parsing the dynamic section and preparing relocation data
    ///
    /// This method processes the dynamic section of the ELF file and prepares
    /// all the data needed for relocation.
    ///
    /// # Returns
    /// The initialized state
    fn init(self) -> Self {
        let lazy_data = match self {
            State::Uninit {
                name,
                dynamic_ptr,
                segments,
                relro,
                user_data,
                lazy_bind,
                init_handler,
                fini_handler,
                phdrs,
                is_dylib,
            } => {
                // If we have a dynamic section, parse it and prepare relocation data
                if let Some(dynamic_ptr) = dynamic_ptr {
                    let dynamic = ElfDynamic::new(dynamic_ptr.as_ptr(), &segments).unwrap();
                    let relocation = DynamicRelocation::new(
                        dynamic.pltrel,
                        dynamic.dynrel,
                        dynamic.relr,
                        dynamic.rel_count,
                    );

                    // Create symbol table from dynamic section
                    let symbols = SymbolTable::from_dynamic(&dynamic);

                    // Collect needed library names
                    let needed_libs: Vec<&'static str> = dynamic
                        .needed_libs
                        .iter()
                        .map(|needed_lib| symbols.strtab().get_str(needed_lib.get()))
                        .collect();

                    // Create the lazy data structure
                    LazyData {
                        extra: ElfExtraData {
                            // Determine if lazy binding should be enabled
                            lazy: lazy_bind.unwrap_or(!dynamic.bind_now),

                            // Store GNU_RELRO segment information
                            relro,

                            // Store relocation information
                            relocation,

                            // Create initialization function
                            init: Box::new(move || {
                                init_handler(dynamic.init_fn, dynamic.init_array_fn)
                            }),

                            // Store GOT pointer
                            got: dynamic.got,

                            // Store RPATH value
                            rpath: dynamic
                                .rpath_off
                                .map(|rpath_off| symbols.strtab().get_str(rpath_off.get())),

                            // Store RUNPATH value
                            runpath: dynamic
                                .runpath_off
                                .map(|runpath_off| symbols.strtab().get_str(runpath_off.get())),
                        },
                        core: CoreComponent {
                            inner: Arc::new(CoreComponentInner {
                                is_init: AtomicBool::new(false),
                                name,
                                symbols: Some(symbols),
                                dynamic: NonNull::new(dynamic.dyn_ptr as _),
                                pltrel: NonNull::new(
                                    dynamic.pltrel.map_or(null(), |plt| plt.as_ptr()) as _,
                                ),
                                phdrs,
                                fini: dynamic.fini_fn,
                                fini_array: dynamic.fini_array_fn,
                                fini_handler,
                                segments,
                                needed_libs: needed_libs.into_boxed_slice(),
                                user_data,
                                lazy_scope: None,
                            }),
                        },
                    }
                } else {
                    // If there's no dynamic section (e.g., for executables)
                    assert!(!is_dylib, "dylib does not have dynamic");
                    let relocation = DynamicRelocation::new(None, None, None, None);

                    // Create minimal lazy data structure
                    LazyData {
                        core: CoreComponent {
                            inner: Arc::new(CoreComponentInner {
                                is_init: AtomicBool::new(false),
                                name,
                                symbols: None,
                                dynamic: None,
                                pltrel: None,
                                phdrs: ElfPhdrs::Mmap(&[]),
                                fini: None,
                                fini_array: None,
                                fini_handler,
                                segments,
                                needed_libs: Box::new([]),
                                user_data,
                                lazy_scope: None,
                            }),
                        },
                        extra: ElfExtraData {
                            lazy: lazy_bind.unwrap_or(false),
                            relro,
                            relocation,
                            init: Box::new(|| {}),
                            got: None,
                            rpath: None,
                            runpath: None,
                        },
                    }
                }
            }
            State::Empty | State::Init(_) => unreachable!(),
        };
        State::Init(lazy_data)
    }
}

/// Lazy parser for ELF object data
///
/// This structure implements lazy parsing of ELF object data, only
/// initializing the data when it's actually needed.
struct LazyParse {
    /// The current state of the parser
    state: Cell<State>,
}

impl LazyParse {
    /// Force initialization of the parser and return a reference to the lazy data
    ///
    /// This method ensures that the ELF object data is initialized and returns
    /// a reference to it. If the data is already initialized, it returns
    /// immediately. Otherwise, it performs the initialization.
    ///
    /// # Returns
    /// A reference to the initialized lazy data
    fn force(&self) -> &LazyData {
        // Fast path - if already initialized, return immediately
        if let State::Init(lazy_data) = unsafe { &*self.state.as_ptr() } {
            return lazy_data;
        }

        // Initialize the data
        self.state.set(self.state.replace(State::Empty).init());

        // Return the initialized data
        match unsafe { &*self.state.as_ptr() } {
            State::Empty | State::Uninit { .. } => unreachable!(),
            State::Init(lazy_data) => lazy_data,
        }
    }

    /// Force initialization of the parser and return a mutable reference to the lazy data
    ///
    /// This method ensures that the ELF object data is initialized and returns
    /// a mutable reference to it. If the data is already initialized, it returns
    /// immediately. Otherwise, it performs the initialization.
    ///
    /// # Returns
    /// A mutable reference to the initialized lazy data
    fn force_mut(&mut self) -> &mut LazyData {
        // Fast path - if already initialized, return immediately
        // 快路径加速
        if let State::Init(lazy_data) = self.state.get_mut() {
            return unsafe { core::mem::transmute(lazy_data) };
        }

        // Initialize the data
        self.state.set(self.state.replace(State::Empty).init());

        // Return the initialized data
        match unsafe { &mut *self.state.as_ptr() } {
            State::Empty | State::Uninit { .. } => unreachable!(),
            State::Init(lazy_data) => lazy_data,
        }
    }
}

impl Deref for LazyParse {
    type Target = LazyData;

    /// Dereference to the lazy data
    ///
    /// This implementation allows direct access to the lazy data through
    /// the LazyParse wrapper.
    #[inline]
    fn deref(&self) -> &LazyData {
        self.force()
    }
}

impl DerefMut for LazyParse {
    /// Mutable dereference to the lazy data
    ///
    /// This implementation allows direct mutable access to the lazy data
    /// through the LazyParse wrapper.
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.force_mut()
    }
}

/// A common part of relocated ELF objects
///
/// This structure represents the common components shared by all relocated
/// ELF objects, whether they are dynamic libraries or executables.
pub struct RelocatedCommonPart {
    /// Entry point of the ELF object
    entry: usize,

    /// PT_INTERP segment value (interpreter path)
    interp: Option<&'static str>,

    /// Name of the ELF file
    name: &'static CStr,

    /// Program headers
    phdrs: ElfPhdrs,

    /// Data parsed lazily
    data: LazyParse,
}

impl RelocatedCommonPart {
    /// Gets the entry point of the ELF object
    ///
    /// # Returns
    /// The virtual address of the entry point
    #[inline]
    pub fn entry(&self) -> usize {
        self.entry
    }

    /// Gets the core component reference of the ELF object
    ///
    /// # Returns
    /// A reference to the CoreComponent
    #[inline]
    pub fn core_component_ref(&self) -> &CoreComponent {
        &self.data.core
    }

    /// Gets the core component of the ELF object
    ///
    /// # Returns
    /// A cloned CoreComponent
    #[inline]
    pub fn core_component(&self) -> CoreComponent {
        self.data.core.clone()
    }

    /// Converts this object into its core component
    ///
    /// # Returns
    /// The CoreComponent
    #[inline]
    pub fn into_core_component(self) -> CoreComponent {
        self.data.force();
        match self.data.state.into_inner() {
            State::Empty | State::Uninit { .. } => unreachable!(),
            State::Init(lazy_data) => lazy_data.core,
        }
    }

    /// Whether lazy binding is enabled for the current ELF object
    ///
    /// # Returns
    /// * `true` - If lazy binding is enabled
    /// * `false` - If lazy binding is disabled
    #[inline]
    pub fn is_lazy(&self) -> bool {
        self.data.extra.lazy
    }

    /// Gets the DT_RPATH value
    ///
    /// # Returns
    /// An optional string slice containing the RPATH value
    #[inline]
    pub fn rpath(&self) -> Option<&str> {
        self.data.extra.rpath
    }

    /// Gets the DT_RUNPATH value
    ///
    /// # Returns
    /// An optional string slice containing the RUNPATH value
    #[inline]
    pub fn runpath(&self) -> Option<&str> {
        self.data.extra.runpath
    }

    /// Gets the PT_INTERP value
    ///
    /// # Returns
    /// An optional string slice containing the interpreter path
    #[inline]
    pub fn interp(&self) -> Option<&str> {
        self.interp
    }

    /// Gets the name of the ELF object
    ///
    /// # Returns
    /// A string slice containing the ELF object name
    #[inline]
    pub fn name(&self) -> &str {
        self.name.to_str().unwrap()
    }

    /// Gets the C-style name of the ELF object
    ///
    /// # Returns
    /// A CStr reference containing the ELF object name
    #[inline]
    pub fn cname(&self) -> &CStr {
        self.name
    }

    /// Gets the short name of the ELF object
    ///
    /// This method returns just the filename portion without any path components.
    ///
    /// # Returns
    /// A string slice containing just the filename
    #[inline]
    pub fn shortname(&self) -> &str {
        self.name().split('/').next_back().unwrap()
    }

    /// Gets the program headers of the ELF object
    ///
    /// # Returns
    /// A slice of the program headers
    pub fn phdrs(&self) -> &[ElfPhdr] {
        match &self.phdrs {
            ElfPhdrs::Mmap(phdrs) => &phdrs,
            ElfPhdrs::Vec(phdrs) => &phdrs,
        }
    }

    /// Gets the Global Offset Table pointer
    ///
    /// # Returns
    /// An optional NonNull pointer to the GOT
    #[inline]
    pub(crate) fn got(&self) -> Option<NonNull<usize>> {
        self.data.extra.got
    }

    /// Gets the dynamic relocation information
    ///
    /// # Returns
    /// A reference to the DynamicRelocation structure
    #[inline]
    pub(crate) fn relocation(&self) -> &DynamicRelocation {
        &self.data.extra.relocation
    }

    /// Marks the ELF object as finished and calls the initialization function
    ///
    /// This method marks the ELF object as fully initialized and calls
    /// any registered initialization functions.
    #[inline]
    pub(crate) fn finish(&self) {
        self.data.core.set_init();
        (self.data.extra.init)();
    }

    /// Gets the GNU_RELRO segment information
    ///
    /// # Returns
    /// An optional reference to the ELFRelro structure
    #[inline]
    pub(crate) fn relro(&self) -> Option<&ELFRelro> {
        self.data.extra.relro.as_ref()
    }

    /// Gets a mutable reference to the user data
    ///
    /// # Returns
    /// An optional mutable reference to the user data
    #[inline]
    pub(crate) fn user_data_mut(&mut self) -> Option<&mut UserData> {
        Arc::get_mut(&mut self.data.core.inner).map(|inner| &mut inner.user_data)
    }

    // Delegate methods to the core component
    delegate! {
        to self.data.core{
            pub(crate) fn symtab(&self) -> Option<&SymbolTable>;
            /// Gets the base address of the ELF object.
            pub fn base(&self) -> usize;
            /// Gets the needed libs' name of the ELF object.
            pub fn needed_libs(&self) -> &[&str];
            /// Gets the address of the dynamic section.
            pub fn dynamic(&self) -> Option<NonNull<Dyn>>;
            /// Gets the memory length of the ELF object map.
            pub fn map_len(&self) -> usize;
            /// Gets user data from the ELF object.
            pub fn user_data(&self) -> &UserData;
        }
    }
}

/// Builder for creating relocated ELF objects
///
/// This structure is used internally during the loading process to collect
/// and organize the various components of a relocated ELF file before
/// building the final RelocatedCommonPart object.
pub(crate) struct RelocatedBuilder<'hook, M: Mmap> {
    /// Optional hook function for processing program headers
    hook: Option<&'hook Hook>,

    /// Mapped program headers
    phdr_mmap: Option<&'static [ElfPhdr]>,

    /// Name of the ELF file
    name: CString,

    /// Lazy binding override setting
    lazy_bind: Option<bool>,

    /// ELF header
    ehdr: ElfHeader,

    /// GNU_RELRO segment information
    relro: Option<ELFRelro>,

    /// Pointer to the dynamic section
    dynamic_ptr: Option<NonNull<Dyn>>,

    /// User-defined data
    user_data: UserData,

    /// Memory segments
    segments: ElfSegments,

    /// Initialization function handler
    init_fn: FnHandler,

    /// Finalization function handler
    fini_fn: FnHandler,

    /// Pointer to the interpreter path (PT_INTERP)
    interp: Option<NonNull<c_char>>,

    /// Phantom data to maintain Mmap type information
    _marker: PhantomData<M>,
}

impl<'hook, M: Mmap> RelocatedBuilder<'hook, M> {
    /// Create a new RelocatedBuilder
    ///
    /// # Arguments
    /// * `hook` - Optional hook function for processing program headers
    /// * `segments` - Memory segments of the ELF file
    /// * `name` - Name of the ELF file
    /// * `lazy_bind` - Lazy binding override setting
    /// * `ehdr` - ELF header
    /// * `init_fn` - Initialization function handler
    /// * `fini_fn` - Finalization function handler
    ///
    /// # Returns
    /// A new RelocatedBuilder instance
    pub(crate) const fn new(
        hook: Option<&'hook Hook>,
        segments: ElfSegments,
        name: CString,
        lazy_bind: Option<bool>,
        ehdr: ElfHeader,
        init_fn: FnHandler,
        fini_fn: FnHandler,
    ) -> Self {
        Self {
            hook,
            phdr_mmap: None,
            name,
            lazy_bind,
            ehdr,
            relro: None,
            dynamic_ptr: None,
            segments,
            user_data: UserData::empty(),
            init_fn,
            fini_fn,
            interp: None,
            _marker: PhantomData,
        }
    }

    /// Parse a program header and extract relevant information
    ///
    /// This method processes a program header and extracts information
    /// needed for relocation, such as the dynamic section, GNU_RELRO
    /// segment, and interpreter path.
    ///
    /// # Arguments
    /// * `phdr` - The program header to parse
    ///
    /// # Returns
    /// * `Ok(())` - If parsing succeeds
    /// * `Err(Error)` - If parsing fails
    fn parse_phdr(&mut self, phdr: &ElfPhdr) -> Result<()> {
        // Execute hook function if provided
        if let Some(hook) = self.hook {
            hook(&self.name, phdr, &self.segments, &mut self.user_data).map_err(|err| {
                parse_phdr_error(
                    format!(
                        "failed to execute the hook function on dylib: {}",
                        self.name.to_str().unwrap()
                    ),
                    err,
                )
            })?;
        }

        // Process different program header types
        match phdr.p_type {
            // Parse the .dynamic section
            PT_DYNAMIC => {
                self.dynamic_ptr =
                    Some(NonNull::new(self.segments.get_mut_ptr(phdr.p_paddr as usize)).unwrap())
            }

            // Store GNU_RELRO segment information
            PT_GNU_RELRO => self.relro = Some(ELFRelro::new::<M>(phdr, self.segments.base())),

            // Store program header table mapping
            PT_PHDR => {
                self.phdr_mmap = Some(
                    self.segments
                        .get_slice::<ElfPhdr>(phdr.p_vaddr as usize, phdr.p_memsz as usize),
                );
            }

            // Store interpreter path
            PT_INTERP => {
                self.interp =
                    Some(NonNull::new(self.segments.get_mut_ptr(phdr.p_vaddr as usize)).unwrap());
            }

            // Ignore other program header types
            _ => {}
        };
        Ok(())
    }

    /// Create program headers from the parsed data
    ///
    /// This method creates the appropriate program header representation
    /// based on whether they are mapped in memory or need to be stored
    /// in a vector.
    ///
    /// # Arguments
    /// * `phdrs` - Slice of program headers
    ///
    /// # Returns
    /// An ElfPhdrs enum containing either mapped or vector-based headers
    fn create_phdrs(&self, phdrs: &[ElfPhdr]) -> ElfPhdrs {
        let (phdr_start, phdr_end) = self.ehdr.phdr_range();

        // Get mapped program headers or create them from loaded segments
        self.phdr_mmap
            .or_else(|| {
                phdrs
                    .iter()
                    .filter(|phdr| phdr.p_type == PT_LOAD)
                    .find_map(|phdr| {
                        let cur_range =
                            phdr.p_offset as usize..(phdr.p_offset + phdr.p_filesz) as usize;
                        if cur_range.contains(&phdr_start) && cur_range.contains(&phdr_end) {
                            return Some(self.segments.get_slice::<ElfPhdr>(
                                phdr.p_vaddr as usize + phdr_start - cur_range.start,
                                self.ehdr.e_phnum() * size_of::<ElfPhdr>(),
                            ));
                        }
                        None
                    })
            })
            .map(|phdrs| ElfPhdrs::Mmap(phdrs))
            .unwrap_or_else(|| ElfPhdrs::Vec(Vec::from(phdrs)))
    }

    /// Build the final RelocatedCommonPart object
    ///
    /// This method completes the building process by parsing all program
    /// headers and constructing the final RelocatedCommonPart object.
    ///
    /// # Arguments
    /// * `phdrs` - Slice of program headers
    ///
    /// # Returns
    /// * `Ok(RelocatedCommonPart)` - The built relocated ELF object
    /// * `Err(Error)` - If building fails
    pub(crate) fn build(mut self, phdrs: &[ElfPhdr]) -> Result<RelocatedCommonPart> {
        // Determine if this is a dynamic library
        let is_dylib = self.ehdr.is_dylib();

        // Parse all program headers
        for phdr in phdrs {
            self.parse_phdr(phdr)?;
        }

        // Create program headers representation
        let phdrs = self.create_phdrs(phdrs);

        // Build and return the relocated common part
        Ok(RelocatedCommonPart {
            entry: self.ehdr.e_entry as usize + if is_dylib { self.segments.base() } else { 0 },
            interp: self
                .interp
                .map(|s| unsafe { CStr::from_ptr(s.as_ptr()).to_str().unwrap() }),
            name: unsafe { core::mem::transmute::<&CStr, &CStr>(self.name.as_c_str()) },
            phdrs: phdrs.clone(),
            data: LazyParse {
                state: Cell::new(State::Uninit {
                    is_dylib,
                    phdrs,
                    init_handler: self.init_fn,
                    fini_handler: self.fini_fn,
                    name: self.name,
                    dynamic_ptr: self.dynamic_ptr,
                    segments: self.segments,
                    relro: self.relro,
                    user_data: self.user_data,
                    lazy_bind: self.lazy_bind,
                }),
            },
        })
    }
}
