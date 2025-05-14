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
    ptr::{NonNull, null_mut},
};
use elf::abi::*;

impl ElfDynamic {
    pub fn new(dynamic_ptr: *const Dyn, segments: &ElfSegments) -> Result<Self> {
        // 这两个是一个格式正常的elf动态库中必须存在的
        let mut symtab_off = 0;
        let mut strtab_off = 0;
        let mut hash_off = None;
        let mut got_off = None;
        let mut pltrel_size = None;
        let mut pltrel_off = None;
        let mut rel_off = None;
        let mut rel_size = None;
        let mut rel_count = None;
        let mut relr_off = None;
        let mut relr_size = None;
        let mut init_off = None;
        let mut fini_off = None;
        let mut init_array_off = None;
        let mut init_array_size = None;
        let mut fini_array_off = None;
        let mut fini_array_size = None;
        let mut version_ids_off = None;
        let mut verneed_off = None;
        let mut verneed_num = None;
        let mut verdef_off = None;
        let mut verdef_num = None;
        let mut rpath_off = None;
        let mut runpath_off = None;
        let mut flags = 0;
        let mut is_rela = None;
        let mut needed_libs = Vec::new();

        let mut cur_dyn_ptr = dynamic_ptr;
        let mut dynamic = unsafe { &*cur_dyn_ptr };
        let base = segments.base();

        unsafe {
            loop {
                match dynamic.d_tag as _ {
                    DT_FLAGS => flags = dynamic.d_un as usize,
                    DT_PLTGOT => got_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize)),
                    DT_NEEDED => {
                        needed_libs.push(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_GNU_HASH => hash_off = Some(dynamic.d_un as usize),
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
        if let Some(is_rela) = is_rela {
            assert!(
                is_rela && size_of::<ElfRelType>() == size_of::<ElfRela>()
                    || !is_rela && size_of::<ElfRelType>() == size_of::<ElfRel>()
            );
        }
        let hash_off = hash_off.ok_or(parse_dynamic_error(
            "dynamic section does not have DT_GNU_HASH",
        ))?;
        let pltrel = pltrel_off
            .map(|pltrel_off| segments.get_slice(pltrel_off.get(), pltrel_size.unwrap().get()));
        let dynrel =
            rel_off.map(|rel_off| segments.get_slice(rel_off.get(), rel_size.unwrap().get()));
        let relr =
            relr_off.map(|relr_off| segments.get_slice(relr_off.get(), relr_size.unwrap().get()));
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
        let verneed = verneed_off.map(|verneed_off| unsafe {
            (
                verneed_off.checked_add(base).unwrap_unchecked(),
                verneed_num.unwrap_unchecked(),
            )
        });
        let verdef = verdef_off.map(|verdef_off| unsafe {
            (
                verdef_off.checked_add(base).unwrap_unchecked(),
                verdef_num.unwrap_unchecked(),
            )
        });
        let version_idx =
            version_ids_off.map(|off| unsafe { off.checked_add(base).unwrap_unchecked() });
        Ok(ElfDynamic {
            dyn_ptr: dynamic_ptr,
            hashtab: hash_off + base,
            symtab: symtab_off + base,
            strtab: strtab_off + base,
            bind_now: flags & DF_BIND_NOW as usize != 0,
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

/// Information in the dynamic section after mapping to the real address
pub struct ElfDynamic {
    pub dyn_ptr: *const Dyn,
    /// DT_GNU_HASH
    pub hashtab: usize,
    /// DT_STMTAB
    pub symtab: usize,
    /// DT_STRTAB
    pub strtab: usize,
    /// DT_FLAGS
    pub bind_now: bool,
    /// DT_PLTGOT
    pub got: Option<NonNull<usize>>,
    /// DT_INIT
    pub init_fn: Option<extern "C" fn()>,
    /// DT_INIT_ARRAY
    pub init_array_fn: Option<&'static [extern "C" fn()]>,
    /// DT_FINI
    pub fini_fn: Option<extern "C" fn()>,
    /// DT_FINI_ARRAY
    pub fini_array_fn: Option<&'static [extern "C" fn()]>,
    /// DT_JMPREL
    pub pltrel: Option<&'static [ElfRelType]>,
    /// DT_RELA
    pub dynrel: Option<&'static [ElfRelType]>,
    /// DT_RELR
    pub relr: Option<&'static [ElfRelr]>,
    /// DT_RELACOUNT
    pub rel_count: Option<NonZeroUsize>,
    /// DT_NEEDED
    pub needed_libs: Vec<NonZeroUsize>,
    /// DT_VERSYM
    pub version_idx: Option<NonZeroUsize>,
    /// DT_VERNEED and DT_VERNEEDNUM
    pub verneed: Option<(NonZeroUsize, NonZeroUsize)>,
    /// DT_VERDEF and DT_VERDEFNUM
    pub verdef: Option<(NonZeroUsize, NonZeroUsize)>,
    /// DT_RPATH
    pub rpath_off: Option<NonZeroUsize>,
    /// DT_RUNPATH
    pub runpath_off: Option<NonZeroUsize>,
}
