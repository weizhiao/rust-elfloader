use crate::{
    Result,
    arch::StaticRelocator,
    elf::ElfRelType,
    image::{ElfCore, LoadedCore, RawObject},
    relocation::SymbolLookup,
    segment::section::PltGotSection,
};
use alloc::{boxed::Box, vec::Vec};

pub(crate) struct StaticRelocation {
    relocation: Box<[&'static [ElfRelType]]>,
}

impl StaticRelocation {
    pub(crate) fn new(relocation: Vec<&'static [ElfRelType]>) -> Self {
        Self {
            relocation: relocation.into_boxed_slice(),
        }
    }
}

impl RawObject {
    pub(crate) fn relocate_impl<PreS, PostS>(
        mut self,
        scope: &[LoadedCore<()>],
        pre_find: &PreS,
        post_find: &PostS,
    ) -> Result<LoadedCore<()>>
    where
        PreS: SymbolLookup + ?Sized,
        PostS: SymbolLookup + ?Sized,
    {
        for reloc in self.relocation.relocation.iter() {
            for rel in *reloc {
                StaticRelocator::relocate(
                    &self.core,
                    rel,
                    &mut self.pltgot,
                    scope,
                    pre_find,
                    post_find,
                )?;
            }
        }
        (self.mprotect)()?;
        (self.init)(None, self.init_array);
        Ok(unsafe { LoadedCore::from_core(self.core) })
    }
}

pub(crate) trait StaticReloc {
    fn relocate<PreS, PostS>(
        core: &ElfCore<()>,
        rel_type: &ElfRelType,
        pltgot: &mut PltGotSection,
        scope: &[LoadedCore<()>],
        pre_find: &PreS,
        post_find: &PostS,
    ) -> Result<()>
    where
        PreS: SymbolLookup + ?Sized,
        PostS: SymbolLookup + ?Sized;

    fn needs_got(_rel_type: u32) -> bool {
        false
    }

    fn needs_plt(_rel_type: u32) -> bool {
        false
    }
}
