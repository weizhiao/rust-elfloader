//! Parsing `.dynamic` section
use crate::{
    arch::{Dyn, ElfRela},
    parse_dynamic_error, Result,
};
use alloc::vec::Vec;
use core::{num::NonZeroUsize, slice::from_raw_parts, usize};
use elf::abi::*;

/// Information in the dynamic section
pub struct ElfRawDynamic {
    pub dyn_ptr: *const Dyn,
    /// DT_GNU_HASH
    pub hash_off: usize,
    /// DT_STMTAB
    pub symtab_off: usize,
    /// DT_STRTAB
    pub strtab_off: usize,
    /// DT_FLAGS
    pub flags: usize,
    /// DT_PLTGOT
    pub got_off: Option<NonZeroUsize>,
    /// DT_JMPREL
    pub pltrel_off: Option<NonZeroUsize>,
    /// DT_PLTRELSZ
    pub pltrel_size: Option<NonZeroUsize>,
    /// DT_RELA
    pub rela_off: Option<NonZeroUsize>,
    /// DT_RELASZ
    pub rela_size: Option<NonZeroUsize>,
    /// DT_RELACOUNT
    pub rela_count: Option<NonZeroUsize>,
    /// DT_INIT
    pub init_off: Option<NonZeroUsize>,
    /// DT_FINI
    pub fini_off: Option<NonZeroUsize>,
    /// DT_INIT_ARRAY
    pub init_array_off: Option<NonZeroUsize>,
    /// DT_INIT_ARRAYSZ
    pub init_array_size: Option<NonZeroUsize>,
    /// DT_FINI_ARRAY
    pub fini_array_off: Option<NonZeroUsize>,
    /// DT_FINI_ARRAYSZ
    pub fini_array_size: Option<NonZeroUsize>,
    /// DT_VERSYM
    pub version_ids_off: Option<NonZeroUsize>,
    /// DT_VERNEED
    pub verneed_off: Option<NonZeroUsize>,
    /// DT_VERNEEDNUM
    pub verneed_num: Option<NonZeroUsize>,
    /// DT_VERDEF
    pub verdef_off: Option<NonZeroUsize>,
    /// DT_VERDEFNUM
    pub verdef_num: Option<NonZeroUsize>,
    /// DT_NEEDED
    pub needed_libs: Vec<NonZeroUsize>,
    /// DT_RPATH
    pub rpath_off: Option<NonZeroUsize>,
    /// DT_RUNPATH
    pub runpath_off: Option<NonZeroUsize>,
}

impl ElfRawDynamic {
    pub fn new(dynamic_ptr: *const Dyn) -> Result<ElfRawDynamic> {
        // 这两个是一个格式正常的elf动态库中必须存在的
        let mut symtab_off = 0;
        let mut strtab_off = 0;
        let mut hash_off = None;
        let mut got_off = None;
        let mut pltrel_size = None;
        let mut pltrel_off = None;
        let mut rela_off = None;
        let mut rela_size = None;
        let mut rela_count = None;
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
        let mut needed_libs = Vec::new();

        let mut cur_dyn_ptr = dynamic_ptr;
        let mut dynamic = unsafe { &*cur_dyn_ptr };

        unsafe {
            loop {
                match dynamic.d_tag {
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
                    DT_JMPREL => {
                        pltrel_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_RELA => rela_off = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize)),
                    DT_RELASZ => {
                        rela_size = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
                    }
                    DT_RELACOUNT => {
                        rela_count = Some(NonZeroUsize::new_unchecked(dynamic.d_un as usize))
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
        let hash_off = hash_off.ok_or(parse_dynamic_error(
            "dynamic section does not have DT_GNU_HASH",
        ))?;
        Ok(ElfRawDynamic {
            dyn_ptr: dynamic_ptr,
            hash_off,
            symtab_off,
            flags,
            got_off,
            needed_libs,
            strtab_off,
            pltrel_off,
            pltrel_size,
            rela_off,
            rela_size,
            init_off,
            fini_off,
            init_array_off,
            init_array_size,
            fini_array_off,
            fini_array_size,
            version_ids_off,
            verneed_off,
            verneed_num,
            verdef_off,
            verdef_num,
            rela_count,
            rpath_off,
            runpath_off,
        })
    }

    /// Map the offset address to an address in actual memory.
    pub fn finish(self, base: usize) -> ElfDynamic {
        let pltrel = self.pltrel_off.map(|pltrel_off| unsafe {
            from_raw_parts(
                (base + pltrel_off.get()) as *const ElfRela,
                self.pltrel_size.unwrap_unchecked().get() / size_of::<ElfRela>(),
            )
        });
        let dynrel = self.rela_off.map(|rel_off| unsafe {
            from_raw_parts(
                (base + rel_off.get()) as *const ElfRela,
                self.rela_size.unwrap_unchecked().get() / size_of::<ElfRela>(),
            )
        });
        let init_fn = self
            .init_off
            .map(|val| unsafe { core::mem::transmute(val.get() + base) });
        let init_array_fn = self.init_array_off.map(|init_array_off| {
            let ptr = init_array_off.get() + base;
            unsafe {
                from_raw_parts(
                    ptr as _,
                    self.init_array_size.unwrap_unchecked().get() / size_of::<usize>(),
                )
            }
        });
        let fini_fn = self
            .fini_off
            .map(|fini_off| unsafe { core::mem::transmute(fini_off.get() + base) });
        let fini_array_fn = self.fini_array_off.map(|fini_array_off| {
            let ptr = fini_array_off.get() + base;
            unsafe {
                from_raw_parts(
                    ptr as _,
                    self.fini_array_size.unwrap_unchecked().get() / size_of::<usize>(),
                )
            }
        });
        let verneed = self.verneed_off.map(|verneed_off| unsafe {
            (
                verneed_off.checked_add(base).unwrap_unchecked(),
                self.verneed_num.unwrap_unchecked(),
            )
        });
        let verdef = self.verdef_off.map(|verdef_off| unsafe {
            (
                verdef_off.checked_add(base).unwrap_unchecked(),
                self.verdef_num.unwrap_unchecked(),
            )
        });
        let version_idx = self
            .version_ids_off
            .map(|off| unsafe { off.checked_add(base).unwrap_unchecked() });
        ElfDynamic {
            dyn_ptr: self.dyn_ptr,
            hashtab: self.hash_off + base,
            symtab: self.symtab_off + base,
            strtab: self.strtab_off + base,
            bind_now: self.flags & DF_BIND_NOW as usize != 0,
            got: self.got_off.map(|off| (base + off.get()) as *mut usize),
            init_fn,
            init_array_fn,
            fini_fn,
            fini_array_fn,
            pltrel,
            dynrel,
            rela_count: self.rela_count,
            needed_libs: self.needed_libs,
            version_idx,
            verneed,
            verdef,
            rpath_off: self.rpath_off,
            runpath_off: self.runpath_off,
        }
    }
}

/// Information in the dynamic section after mapping to the real address
pub struct ElfDynamic {
    pub dyn_ptr: *const Dyn,
    pub hashtab: usize,
    pub symtab: usize,
    pub strtab: usize,
    pub bind_now: bool,
    pub got: Option<*mut usize>,
    pub init_fn: Option<extern "C" fn()>,
    pub init_array_fn: Option<&'static [extern "C" fn()]>,
    pub fini_fn: Option<extern "C" fn()>,
    pub fini_array_fn: Option<&'static [extern "C" fn()]>,
    pub pltrel: Option<&'static [ElfRela]>,
    pub dynrel: Option<&'static [ElfRela]>,
    pub rela_count: Option<NonZeroUsize>,
    pub needed_libs: Vec<NonZeroUsize>,
    pub version_idx: Option<NonZeroUsize>,
    pub verneed: Option<(NonZeroUsize, NonZeroUsize)>,
    pub verdef: Option<(NonZeroUsize, NonZeroUsize)>,
    pub rpath_off: Option<NonZeroUsize>,
    pub runpath_off: Option<NonZeroUsize>,
}
