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
use alloc::sync::Arc;
use alloc::{boxed::Box, ffi::CString, vec::Vec};
use elf::abi::{SHT_REL, SHT_RELA, SHT_SYMTAB};
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

impl<M: Mmap> Loader<M> {
    pub fn load_relocatable(&mut self, mut object: impl ElfObject) -> Result<ElfRelocatable> {
        let ehdr = self.buf.prepare_ehdr(&mut object).unwrap();
        self.load_rel(ehdr, object)
    }
}

pub(crate) struct RelocatableBuilder {
    name: CString,
    symtab: Option<SymbolTable>,
    init_fn: FnHandler,
    fini_fn: FnHandler,
    segments: ElfSegments,
    relocation: StaticRelocation,
    mprotect: Box<dyn Fn() -> Result<()>>,
}

impl RelocatableBuilder {
    pub(crate) fn new(
        name: CString,
        shdrs: &mut [ElfShdr],
        init_fn: FnHandler,
        fini_fn: FnHandler,
        segments: ElfSegments,
        mprotect: Box<dyn Fn() -> Result<()>>,
    ) -> Self {
        let base = segments.base();
        shdrs
            .iter_mut()
            .for_each(|shdr| shdr.sh_addr = (shdr.sh_addr as usize + base) as _);
        let mut symtab = None;
        for shdr in shdrs.iter() {
            match shdr.sh_type {
                SHT_SYMTAB => {
                    symtab = Some(SymbolTable::from_shdrs(base, &shdr, shdrs));
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
            relocation: StaticRelocation::new(shdrs),
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
            mprotect: self.mprotect,
        }
    }
}

pub struct ElfRelocatable {
    pub(crate) core: CoreComponent,
    pub(crate) relocation: StaticRelocation,
    pub(crate) mprotect: Box<dyn Fn() -> Result<()>>,
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
