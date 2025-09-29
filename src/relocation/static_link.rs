use crate::{
    Result,
    arch::{ElfRelType, ElfShdr},
    format::{Relocated, relocatable::ElfRelocatable},
};
use alloc::{boxed::Box, vec::Vec};
use elf::abi::{SHT_REL, SHT_RELA};

pub(crate) struct StaticRelocation {
    relocation: Box<[&'static [ElfRelType]]>,
}

impl StaticRelocation {
    pub(crate) fn new(shdrs: &[&ElfShdr]) -> Self {
        let mut relocation = Vec::with_capacity(shdrs.len());
        for shdr in shdrs {
            debug_assert!(shdr.sh_type == SHT_REL || shdr.sh_type == SHT_RELA);
            relocation.push(shdr.content());
        }
        Self {
            relocation: relocation.into_boxed_slice(),
        }
    }
}

impl ElfRelocatable {
    pub(crate) fn relocate_impl<'lib, 'iter, 'find, F>(
        &self,
        scope: &[&'iter Relocated],
        pre_find: &'find F,
    ) -> Result<Relocated<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        'iter: 'lib,
        'find: 'lib,
    {
        for reloc in self.relocation.relocation.iter() {
            for rel in *reloc {}
        }
        todo!()
    }
}
