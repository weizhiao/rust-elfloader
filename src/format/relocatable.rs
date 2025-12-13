//! Relocatable ELF file handling
//!
//! This module provides functionality for loading and relocating relocatable
//! ELF files (also known as object files). These are typically .o files that
//! contain code and data that need to be relocated before they can be executed.

use core::{fmt::Debug, ops::Deref, sync::atomic::AtomicBool};

use crate::{
    CoreComponent, Hook, Loader, Result, UserData,
    arch::{ElfRelType, ElfShdr, ElfSymbol},
    format::{CoreComponentInner, ElfType, Relocated},
    loader::FnHandler,
    mmap::Mmap,
    object::ElfObject,
    relocation::{
        Relocatable, RelocationHandler, SymbolLookup, dynamic_link::LazyScope,
        static_link::StaticRelocation,
    },
    segment::{ElfSegments, shdr::PltGotSection},
    symbol::SymbolTable,
};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
use alloc::{boxed::Box, ffi::CString, vec::Vec};
use elf::abi::{SHT_INIT_ARRAY, SHT_REL, SHT_RELA, SHT_SYMTAB, STT_FILE};
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

impl<M: Mmap, H: Hook> Loader<M, H> {
    /// Load a relocatable ELF file into memory
    ///
    /// This method loads a relocatable ELF file (typically a .o file) into memory
    /// and prepares it for relocation. The file is not yet relocated after this
    /// operation.
    ///
    /// # Arguments
    /// * `object` - The ELF object to load
    /// * `lazy_bind` - Optional override for lazy binding behavior
    ///
    /// # Returns
    /// * `Ok(ElfRelocatable)` - The loaded relocatable ELF file
    /// * `Err(Error)` - If loading fails
    pub fn load_relocatable(&mut self, mut object: impl ElfObject) -> Result<ElfRelocatable> {
        let ehdr = self.buf.prepare_ehdr(&mut object).unwrap();
        self.load_rel(ehdr, object)
    }
}

/// Builder for creating relocatable ELF objects
///
/// This structure is used internally during the loading process to collect
/// and organize the various components of a relocatable ELF file before
/// building the final ElfRelocatable object.
pub(crate) struct RelocatableBuilder {
    /// Name of the ELF file
    name: CString,

    /// Symbol table for the ELF file
    symtab: Option<SymbolTable>,

    /// Initialization function array
    init_array: Option<&'static [fn()]>,

    /// Initialization function handler
    init_fn: FnHandler,

    /// Finalization function handler
    fini_fn: FnHandler,

    /// Memory segments of the ELF file
    segments: ElfSegments,

    /// Static relocation information
    relocation: StaticRelocation,

    /// Memory protection function
    mprotect: Box<dyn Fn() -> Result<()>>,

    /// PLT/GOT section information
    pltgot: PltGotSection,
}

impl RelocatableBuilder {
    /// Create a new RelocatableBuilder
    ///
    /// This method initializes a new RelocatableBuilder with the provided
    /// components and processes the section headers to prepare for relocation.
    ///
    /// # Arguments
    /// * `name` - The name of the ELF file
    /// * `shdrs` - Mutable reference to the section headers
    /// * `init_fn` - Initialization function handler
    /// * `fini_fn` - Finalization function handler
    /// * `segments` - Memory segments of the ELF file
    /// * `mprotect` - Memory protection function
    /// * `pltgot` - PLT/GOT section information
    ///
    /// # Returns
    /// A new RelocatableBuilder instance
    pub(crate) fn new(
        name: CString,
        shdrs: &mut [ElfShdr],
        init_fn: FnHandler,
        fini_fn: FnHandler,
        segments: ElfSegments,
        mprotect: Box<dyn Fn() -> Result<()>>,
        mut pltgot: PltGotSection,
    ) -> Self {
        // Calculate the base address for relocations
        let base = segments.base();

        // Update section header addresses with the base offset
        shdrs
            .iter_mut()
            .for_each(|shdr| shdr.sh_addr = (shdr.sh_addr as usize + base) as _);

        // Rebase and initialize the PLT/GOT section
        pltgot.rebase(base);

        // Initialize optional components
        let mut symtab = None;
        let mut relocation = Vec::with_capacity(shdrs.len());
        let mut init_array = None;

        // Process each section header
        for shdr in shdrs.iter() {
            match shdr.sh_type {
                // Symbol table section
                SHT_SYMTAB => {
                    let symbols: &mut [ElfSymbol] = shdr.content_mut();
                    // Update symbol values with section base offsets
                    for symbol in symbols.iter_mut() {
                        if symbol.st_type() == STT_FILE {
                            continue;
                        }
                        let section_base = shdrs[symbol.st_shndx()].sh_addr as usize - base;
                        symbol.set_value(section_base + symbol.st_value());
                    }
                    // Create symbol table from section headers
                    symtab = Some(SymbolTable::from_shdrs(&shdr, shdrs));
                }

                // Relocation sections (REL or RELA)
                SHT_RELA | SHT_REL => {
                    let rels: &mut [ElfRelType] = shdr.content_mut();
                    // Calculate section base for relocation offsets
                    let section_base = shdrs[shdr.sh_info as usize].sh_addr as usize;
                    // Update relocation offsets with section base
                    for rel in rels.iter_mut() {
                        rel.set_offset(section_base + rel.r_offset() - base);
                    }
                    // Add relocation data to the relocation vector
                    relocation.push(shdr.content());
                }

                // Initialization array section
                SHT_INIT_ARRAY => {
                    let array: &[usize] = shdr.content_mut();
                    // Transmute the array to function pointers
                    init_array = Some(unsafe { core::mem::transmute(array) });
                }

                // Other section types are ignored
                _ => {}
            }
        }

        // Construct and return the builder
        Self {
            name,
            symtab,
            init_fn,
            fini_fn,
            segments,
            mprotect,
            relocation: StaticRelocation::new(relocation),
            pltgot,
            init_array,
        }
    }

    /// Build the final ElfRelocatable object
    ///
    /// This method constructs the final ElfRelocatable object from the
    /// components collected during the building process.
    ///
    /// # Returns
    /// An ElfRelocatable instance ready for relocation
    pub(crate) fn build(self) -> ElfRelocatable {
        // Create the inner component structure
        let inner = CoreComponentInner {
            is_init: AtomicBool::new(false),
            name: self.name,
            symbols: self.symtab,
            dynamic_info: None,
            fini: None,
            fini_array: None,
            fini_handler: self.fini_fn,
            user_data: UserData::empty(),
            segments: self.segments,
            elf_type: ElfType::Relocatable,
        };

        // Construct and return the ElfRelocatable object
        ElfRelocatable {
            core: CoreComponent {
                inner: Arc::new(inner),
            },
            pltgot: self.pltgot,
            relocation: self.relocation,
            mprotect: self.mprotect,
            init_array: self.init_array,
            init: self.init_fn,
        }
    }
}

/// A relocatable ELF object
///
/// This structure represents a relocatable ELF file (typically a .o file)
/// that has been loaded into memory and is ready for relocation. It contains
/// all the necessary information to perform the relocation process.
pub struct ElfRelocatable {
    /// Core component containing basic ELF information
    pub(crate) core: CoreComponent,

    /// Static relocation information
    pub(crate) relocation: StaticRelocation,

    /// PLT/GOT section information
    pub(crate) pltgot: PltGotSection,

    /// Memory protection function
    pub(crate) mprotect: Box<dyn Fn() -> Result<()>>,

    /// Initialization function handler
    pub(crate) init: FnHandler,

    /// Initialization function array
    pub(crate) init_array: Option<&'static [fn()]>,
}

impl Deref for ElfRelocatable {
    type Target = CoreComponent;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl Debug for ElfRelocatable {
    /// Formats the ElfRelocatable for debugging purposes
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ElfRelocatable")
            .field("core", &self.core)
            .finish()
    }
}

impl Relocatable for ElfRelocatable {
    type Output = Relocated;

    fn relocate<S, PreH, PostH>(
        self,
        scope: &[Relocated],
        pre_find: &S,
        _pre_handler: PreH,
        _post_handler: PostH,
        _lazy: Option<bool>,
        _lazy_scope: Option<LazyScope>,
        _use_scope_as_lazy: bool,
    ) -> Result<Self::Output>
    where
        S: SymbolLookup + ?Sized,
        PreH: RelocationHandler,
        PostH: RelocationHandler,
    {
        self.relocate_impl(scope, pre_find)
    }
}
