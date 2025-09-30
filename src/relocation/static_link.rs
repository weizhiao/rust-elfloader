use core::marker::PhantomData;

use crate::{
    CoreComponent, Result,
    arch::{ElfRelType, ElfShdr, StaticRelocator},
    format::{Relocated, relocatable::ElfRelocatable},
};
use alloc::{boxed::Box, vec::Vec};
use elf::abi::{SHT_REL, SHT_RELA};

pub(crate) struct StaticRelocation {
    relocation: Box<[(&'static [ElfRelType], usize)]>,
}

impl StaticRelocation {
    pub(crate) fn new(shdrs: &[ElfShdr]) -> Self {
        let mut relocation = Vec::with_capacity(shdrs.len());
        for shdr in shdrs {
            if shdr.sh_type != SHT_REL && shdr.sh_type != SHT_RELA {
                continue;
            }
            let p = shdrs[shdr.sh_info as usize].sh_addr as usize;
            relocation.push((shdr.content(), p));
        }
        Self {
            relocation: relocation.into_boxed_slice(),
        }
    }
}

impl ElfRelocatable {
    pub(crate) fn relocate_impl<'lib, 'iter, 'find, F>(
        self,
        scope: &[&'iter Relocated],
        pre_find: &'find F,
    ) -> Result<Relocated<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        'iter: 'lib,
        'find: 'lib,
    {
        for (reloc, target_base) in self.relocation.relocation.iter() {
            for rel in *reloc {
                StaticRelocator::relocate(&self.core, rel, 0, *target_base, scope, pre_find)?;
            }
        }
        (self.mprotect)()?;
        Ok(Relocated {
            core: self.core,
            _marker: PhantomData,
        })
    }
}

pub(crate) trait StaticReloc {
    fn relocate<F>(
        core: &CoreComponent,
        rel_type: &ElfRelType,
        l: usize,
        target_base: usize,
        scope: &[&Relocated],
        pre_find: &F,
    ) -> Result<()>
    where
        F: Fn(&str) -> Option<*const ()>;
}
