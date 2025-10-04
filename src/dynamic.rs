//! Parsing `.dynamic` section
use crate::{
    Result,
    arch::{DT_RELR, DT_RELRSZ, Dyn, ElfRel, ElfRelType, ElfRela, ElfRelr},
    parse_dynamic_error,
    segment::ElfSegments,
};
use alloc::vec::Vec;
use core::{
    num::NonZeroUsize,
    ops::{Add, AddAssign, Sub, SubAssign},
    ptr::{NonNull, null_mut},
};
use elf::abi::*;

impl ElfDynamic {
    /// Parse the dynamic section of an ELF file
    pub fn new(dynamic_ptr: *const Dyn, segments: &ElfSegments) -> Result<Self> {
        // These are required fields in a valid ELF dynamic library
        let mut symtab_off = 0; // Symbol table offset
        let mut strtab_off = 0; // String table offset
        let mut elf_hash_off = None; // ELF hash table offset
        let mut gnu_hash_off = None; // GNU hash table offset
        let mut got_off = None; // Global Offset Table offset
        let mut pltrel_size = None; // PLT relocation table size
        let mut pltrel_off = None; // PLT relocation table offset
        let mut rel_off = None; // Relocation table offset
        let mut rel_size = None; // Relocation table size
        let mut rel_count = None; // Relocation count
        let mut relr_off = None; // RELR relocation table offset
        let mut relr_size = None; // RELR relocation table size
        let mut init_off = None; // Initialization function offset
        let mut fini_off = None; // Finalization function offset
        let mut init_array_off = None; // Initialization function array offset
        let mut init_array_size = None; // Initialization function array size
        let mut fini_array_off = None; // Finalization function array offset
        let mut fini_array_size = None; // Finalization function array size
        let mut version_ids_off = None; // Symbol versioning information offset
        let mut verneed_off = None; // Version needed section offset
        let mut verneed_num = None; // Number of version needed entries
        let mut verdef_off = None; // Version definition section offset
        let mut verdef_num = None; // Number of version definition entries
        let mut rpath_off = None; // Runtime library search path offset
        let mut runpath_off = None; // Runtime library search path offset (overrides RPATH)
        let mut flags = 0; // Dynamic section flags
        let mut flags_1 = 0; // Additional dynamic section flags
        let mut is_rela = None; // Indicates if RELA or REL relocations are used
        let mut needed_libs = Vec::new(); // Required libraries (dependencies)

        let mut cur_dyn_ptr = dynamic_ptr;
        let mut dynamic = unsafe { &*cur_dyn_ptr };
        let base = segments.base();

        // Parse all dynamic entries
        unsafe {
            loop {
                match dynamic.d_tag as _ {
                    DT_FLAGS => flags = dynamic.d_un as usize,
                    DT_FLAGS_1 => flags_1 = dynamic.d_un as usize,
                    DT_PLTGOT => got_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize)),
                    DT_NEEDED => {
                        needed_libs.push(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_HASH => elf_hash_off = Some(dynamic.d_un as usize),
                    DT_GNU_HASH => gnu_hash_off = Some(dynamic.d_un as usize),
                    DT_SYMTAB => symtab_off = dynamic.d_un as usize,
                    DT_STRTAB => strtab_off = dynamic.d_un as usize,
                    DT_PLTRELSZ => {
                        pltrel_size = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_PLTREL => {
                        is_rela = Some(dynamic.d_un as i64 == DT_RELA);
                    }
                    DT_JMPREL => {
                        pltrel_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_RELR => relr_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize)),
                    DT_RELA | DT_REL => {
                        is_rela = Some(dynamic.d_tag as i64 == DT_RELA);
                        rel_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_RELASZ | DT_RELSZ => {
                        rel_size = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_RELRSZ => {
                        relr_size = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_RELACOUNT | DT_RELCOUNT => {
                        rel_count = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_INIT => init_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize)),
                    DT_FINI => fini_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize)),
                    DT_INIT_ARRAY => {
                        init_array_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_INIT_ARRAYSZ => {
                        init_array_size = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_FINI_ARRAY => {
                        fini_array_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_FINI_ARRAYSZ => {
                        fini_array_size = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_VERSYM => {
                        version_ids_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_VERNEED => {
                        verneed_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_VERNEEDNUM => {
                        verneed_num = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_VERDEF => {
                        verdef_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_VERDEFNUM => {
                        verdef_num = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_RPATH => {
                        rpath_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_RUNPATH => {
                        runpath_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_NULL => break,
                    _ => {}
                }
                cur_dyn_ptr = cur_dyn_ptr.add(1);
                dynamic = &*cur_dyn_ptr;
            }
        }

        // Verify relocation type consistency
        if let Some(is_rela) = is_rela {
            assert!(
                is_rela && size_of::<ElfRelType>() == size_of::<ElfRela>()
                    || !is_rela && size_of::<ElfRelType>() == size_of::<ElfRel>()
            );
        }

        // Determine which hash table to use (prefer GNU hash)
        let hash_off = if let Some(off) = gnu_hash_off {
            ElfDynamicHashTab::Gnu(off)
        } else if let Some(off) = elf_hash_off {
            ElfDynamicHashTab::Elf(off)
        } else {
            return Err(parse_dynamic_error(
                "dynamic section does not have DT_GNU_HASH nor DT_HASH",
            ));
        };

        // Extract relocation tables
        let pltrel = pltrel_off
            .map(|pltrel_off| segments.get_slice(pltrel_off.get(), pltrel_size.unwrap().get()));
        let dynrel =
            rel_off.map(|rel_off| segments.get_slice(rel_off.get(), rel_size.unwrap().get()));
        let relr =
            relr_off.map(|relr_off| segments.get_slice(relr_off.get(), relr_size.unwrap().get()));

        // Extract initialization and finalization functions
        let init_fn = init_off
            .map(|val| unsafe { core::mem::transmute(segments.get_ptr::<fn()>(val.get())) });
        let init_array_fn = init_array_off.map(|init_array_off| {
            segments.get_slice(init_array_off.get(), init_array_size.unwrap().get())
        });
        let fini_fn = fini_off.map(|fini_off| unsafe {
            core::mem::transmute(segments.get_ptr::<fn()>(fini_off.get()))
        });
        let fini_array_fn = fini_array_off.map(|fini_array_off| {
            segments.get_slice(fini_array_off.get(), fini_array_size.unwrap().get())
        });

        // Extract versioning information
        let verneed = verneed_off
            .map(|verneed_off| (verneed_off.checked_add(base).unwrap(), verneed_num.unwrap()));
        let verdef = verdef_off
            .map(|verdef_off| (verdef_off.checked_add(base).unwrap(), verdef_num.unwrap()));
        let version_idx = version_ids_off.map(|off| off.checked_add(base).unwrap());

        Ok(ElfDynamic {
            dyn_ptr: dynamic_ptr,
            hashtab: hash_off + base,
            symtab: symtab_off + base,
            strtab: strtab_off + base,
            // Check if binding should be done immediately
            bind_now: flags & DF_BIND_NOW as usize != 0 || flags_1 & DF_1_NOW as usize != 0,
            got: NonNull::new(
                got_off
                    .map(|off| (base + off.get()) as *mut usize)
                    .unwrap_or(null_mut()),
            ),
            needed_libs,
            pltrel,
            dynrel,
            relr,
            init_fn,
            init_array_fn,
            fini_fn,
            fini_array_fn,
            rel_count,
            rpath_off,
            runpath_off,
            version_idx,
            verneed,
            verdef,
        })
    }
}

/// Hash table type used for symbol lookup
pub enum ElfDynamicHashTab {
    /// GNU-style hash table (DT_GNU_HASH)
    Gnu(usize),
    /// Traditional ELF hash table (DT_HASH)
    Elf(usize),
}

impl Into<usize> for ElfDynamicHashTab {
    fn into(self) -> usize {
        match self {
            ElfDynamicHashTab::Gnu(off) => off,
            ElfDynamicHashTab::Elf(off) => off,
        }
    }
}

impl Add<usize> for ElfDynamicHashTab {
    type Output = ElfDynamicHashTab;

    fn add(self, rhs: usize) -> Self::Output {
        match self {
            ElfDynamicHashTab::Gnu(off) => ElfDynamicHashTab::Gnu(off + rhs),
            ElfDynamicHashTab::Elf(off) => ElfDynamicHashTab::Elf(off + rhs),
        }
    }
}

impl Sub<usize> for ElfDynamicHashTab {
    type Output = ElfDynamicHashTab;

    fn sub(self, rhs: usize) -> Self::Output {
        match self {
            ElfDynamicHashTab::Gnu(off) => ElfDynamicHashTab::Gnu(off - rhs),
            ElfDynamicHashTab::Elf(off) => ElfDynamicHashTab::Elf(off - rhs),
        }
    }
}

impl AddAssign<usize> for ElfDynamicHashTab {
    fn add_assign(&mut self, rhs: usize) {
        match self {
            ElfDynamicHashTab::Gnu(off) => *off += rhs,
            ElfDynamicHashTab::Elf(off) => *off += rhs,
        }
    }
}

impl SubAssign<usize> for ElfDynamicHashTab {
    fn sub_assign(&mut self, rhs: usize) {
        match self {
            ElfDynamicHashTab::Gnu(off) => *off -= rhs,
            ElfDynamicHashTab::Elf(off) => *off -= rhs,
        }
    }
}

/// Information in the dynamic section after mapping to the real address
pub struct ElfDynamic {
    pub dyn_ptr: *const Dyn,
    /// DT_GNU_HASH or DT_HASH
    pub hashtab: ElfDynamicHashTab,
    /// DT_SYMTAB - Symbol table address
    pub symtab: usize,
    /// DT_STRTAB - String table address
    pub strtab: usize,
    /// DT_FLAGS and DT_FLAGS_1 - Indicates if all symbols should be bound immediately
    pub bind_now: bool,
    /// DT_PLTGOT - Global Offset Table
    pub got: Option<NonNull<usize>>,
    /// DT_INIT - Initialization function
    pub init_fn: Option<fn()>,
    /// DT_INIT_ARRAY - Initialization function array
    pub init_array_fn: Option<&'static [fn()]>,
    /// DT_FINI - Finalization function
    pub fini_fn: Option<fn()>,
    /// DT_FINI_ARRAY - Finalization function array
    pub fini_array_fn: Option<&'static [fn()]>,
    /// DT_JMPREL - PLT relocation entries
    pub pltrel: Option<&'static [ElfRelType]>,
    /// DT_RELA or DT_REL - Relocation entries
    pub dynrel: Option<&'static [ElfRelType]>,
    /// DT_RELR - RELR relocation entries (for relative relocations)
    pub relr: Option<&'static [ElfRelr]>,
    /// DT_RELACOUNT or DT_RELCOUNT - Count of RELATIVE relocations
    pub rel_count: Option<NonZeroUsize>,
    /// DT_NEEDED - Required libraries (dependencies)
    pub needed_libs: Vec<NonZeroUsize>,
    /// DT_VERSYM - Symbol version information
    pub version_idx: Option<NonZeroUsize>,
    /// DT_VERNEED and DT_VERNEEDNUM - Version needed information
    pub verneed: Option<(NonZeroUsize, NonZeroUsize)>,
    /// DT_VERDEF and DT_VERDEFNUM - Version definition information
    pub verdef: Option<(NonZeroUsize, NonZeroUsize)>,
    /// DT_RPATH - Runtime library search path
    pub rpath_off: Option<NonZeroUsize>,
    /// DT_RUNPATH - Runtime library search path (overrides RPATH)
    pub runpath_off: Option<NonZeroUsize>,
}
