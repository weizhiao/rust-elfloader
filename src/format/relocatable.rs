use core::sync::atomic::AtomicBool;

use crate::{
    CoreComponent, Loader, Result, UserData,
    arch::ElfShdr,
    format::{CoreComponentInner, ElfPhdrs, Relocated},
    loader::FnHandler,
    mmap::Mmap,
    object::ElfObject,
    relocation::static_link::StaticRelocation,
    segment::ElfSegments,
    symbol::SymbolTable,
};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::{Arc, Weak};
use alloc::{boxed::Box, ffi::CString, vec::Vec};
use elf::abi::{SHT_REL, SHT_SYMTAB};
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::{Arc, Weak};

impl<M: Mmap> Loader<M> {
    pub fn load_relocatable(&mut self, mut object: impl ElfObject) -> Result<()> {
        let ehdr = self.buf.prepare_ehdr(&mut object).unwrap();
        self.load_rel(ehdr, object)?;
        Ok(())
    }
}

pub(crate) struct RelocatableBuilder {
    name: CString,
    symtab: Option<SymbolTable>,
    init_fn: FnHandler,
    fini_fn: FnHandler,
    segments: ElfSegments,
    relocation: StaticRelocation,
}

impl RelocatableBuilder {
    pub(crate) fn new(
        name: CString,
        shdrs: &mut [ElfShdr],
        init_fn: FnHandler,
        fini_fn: FnHandler,
        segments: ElfSegments,
    ) -> Self {
        let base = segments.base();
        shdrs
            .iter_mut()
            .for_each(|shdr| shdr.sh_addr = (shdr.sh_addr as usize + base) as _);
        let mut relocations = Vec::new();
        let mut symtab = None;
        for shdr in shdrs.iter() {
            match shdr.sh_type {
                SHT_REL => relocations.push(shdr),
                SHT_SYMTAB => {
                    symtab = Some(SymbolTable::from_shdrs(&shdr, shdrs));
                }
                _ => {}
            }
        }
        Self {
            name,
            symtab,
            init_fn,
            fini_fn,
            segments,
            relocation: StaticRelocation::new(&relocations),
        }
    }

    pub(crate) fn build(self) -> ElfRelocatable {
        let inner = CoreComponentInner {
            is_init: AtomicBool::new(false),
            name: self.name,
            symbols: self.symtab,
            dynamic: None,
            pltrel: None,
            phdrs: ElfPhdrs::Mmap(&[]),
            fini: None,
            fini_array: None,
            fini_handler: self.fini_fn,
            needed_libs: Box::new([]),
            user_data: UserData::empty(),
            lazy_scope: None,
            segments: self.segments,
        };
        ElfRelocatable {
            core: CoreComponent {
                inner: Arc::new(inner),
            },
            relocation: self.relocation,
        }
    }
}

pub struct ElfRelocatable {
    core: CoreComponent,
    pub(crate) relocation: StaticRelocation,
}

impl ElfRelocatable {
    // pub fn relocate<'iter, 'scope, 'find, 'lib, F>(
    //     self,
    //     scope: impl AsRef<[&'iter RelocatedDylib<'scope>]>,
    //     pre_find: &'find F,
    //     deal_unknown: &mut UnknownHandler,
    //     local_lazy_scope: Option<LazyScope<'lib>>,
    // ) -> Result<RelocatedDylib<'lib>>
    // where
    //     F: Fn(&str) -> Option<*const ()>,
    //     'scope: 'iter,
    //     'iter: 'lib,
    //     'find: 'lib,
    // {
    //     Ok(RelocatedDylib {
    //         inner: relocate_impl(
    //             self.inner,
    //             scope.as_ref(),
    //             pre_find,
    //             deal_unknown,
    //             local_lazy_scope,
    //         )?,
    //     })
    // }
}
