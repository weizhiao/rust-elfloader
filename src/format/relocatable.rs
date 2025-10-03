use core::{fmt::Debug, sync::atomic::AtomicBool};

use crate::{
    CoreComponent, Loader, Result, UserData,
    arch::{ElfRelType, ElfShdr, ElfSymbol},
    format::{CoreComponentInner, ElfPhdrs, Relocated},
    loader::FnHandler,
    mmap::Mmap,
    object::ElfObject,
    relocation::static_link::StaticRelocation,
    segment::{ElfSegments, shdr::PltGotSection},
    symbol::SymbolTable,
};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
use alloc::{boxed::Box, ffi::CString, vec::Vec};
use elf::abi::{SHT_INIT_ARRAY, SHT_REL, SHT_RELA, SHT_SYMTAB, STT_FILE};
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

impl<M: Mmap> Loader<M> {
    pub fn load_relocatable(
        &mut self,
        mut object: impl ElfObject,
        lazy_bind: Option<bool>,
    ) -> Result<ElfRelocatable> {
        let ehdr = self.buf.prepare_ehdr(&mut object).unwrap();
        self.load_rel(ehdr, object, lazy_bind)
    }
}

pub(crate) struct RelocatableBuilder {
    name: CString,
    symtab: Option<SymbolTable>,
    init_array: Option<&'static [fn()]>,
    init_fn: FnHandler,
    fini_fn: FnHandler,
    segments: ElfSegments,
    relocation: StaticRelocation,
    mprotect: Box<dyn Fn() -> Result<()>>,
    pltgot: PltGotSection,
}

impl RelocatableBuilder {
    pub(crate) fn new(
        name: CString,
        shdrs: &mut [ElfShdr],
        init_fn: FnHandler,
        fini_fn: FnHandler,
        segments: ElfSegments,
        mprotect: Box<dyn Fn() -> Result<()>>,
        mut pltgot: PltGotSection,
    ) -> Self {
        let base = segments.base();
        shdrs
            .iter_mut()
            .for_each(|shdr| shdr.sh_addr = (shdr.sh_addr as usize + base) as _);
        pltgot.rebase(base);
        pltgot.init_pltgot();
        let mut symtab = None;
        let mut relocation = Vec::with_capacity(shdrs.len());
        let mut init_array = None;
        for shdr in shdrs.iter() {
            match shdr.sh_type {
                SHT_SYMTAB => {
                    let symbols: &mut [ElfSymbol] = shdr.content_mut();
                    for symbol in symbols.iter_mut() {
                        if symbol.st_type() == STT_FILE {
                            continue;
                        }
                        let section_base = shdrs[symbol.st_shndx()].sh_addr as usize - base;
                        symbol.set_value(section_base + symbol.st_value());
                    }
                    symtab = Some(SymbolTable::from_shdrs(&shdr, shdrs));
                }
                SHT_RELA | SHT_REL => {
                    let rels: &mut [ElfRelType] = shdr.content_mut();
                    let section_base = shdrs[shdr.sh_info as usize].sh_addr as usize;
                    for rel in rels.iter_mut() {
                        rel.set_offset(section_base + rel.r_offset() - base);
                    }
                    relocation.push(shdr.content());
                }
                SHT_INIT_ARRAY => {
                    let array: &[usize] = shdr.content_mut();
                    init_array = Some(unsafe { core::mem::transmute(array) });
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
            mprotect,
            relocation: StaticRelocation::new(relocation),
            pltgot,
            init_array,
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
            pltgot: self.pltgot,
            relocation: self.relocation,
            mprotect: self.mprotect,
            init_array: self.init_array,
            init: self.init_fn,
        }
    }
}

pub struct ElfRelocatable {
    pub(crate) core: CoreComponent,
    pub(crate) relocation: StaticRelocation,
    pub(crate) pltgot: PltGotSection,
    pub(crate) mprotect: Box<dyn Fn() -> Result<()>>,
    pub(crate) init: FnHandler,
    pub(crate) init_array: Option<&'static [fn()]>,
}

impl Debug for ElfRelocatable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ElfRelocatable")
            .field("core", &self.core)
            .finish()
    }
}

impl ElfRelocatable {
    pub fn relocate<'iter, 'scope, 'find, 'lib, F>(
        self,
        scope: impl AsRef<[&'iter Relocated<'scope>]>,
        pre_find: &'find F,
    ) -> Result<Relocated<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        let object = self.relocate_impl(scope.as_ref(), pre_find)?;
        Ok(object)
    }
}
