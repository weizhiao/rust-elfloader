use crate::{
    ElfModule, Result,
    arch::{ElfRelType, StaticRelocator},
    format::{LoadedModule, ObjectImage},
    relocation::SymbolLookup,
    segment::shdr::PltGotSection,
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

impl ObjectImage {
    pub(crate) fn relocate_impl<PreS, PostS>(
        mut self,
        scope: &[LoadedModule<()>],
        pre_find: &PreS,
        post_find: &PostS,
    ) -> Result<LoadedModule<()>>
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
        Ok(unsafe { LoadedModule::from_core(self.core) })
    }
}

pub(crate) trait StaticReloc {
    fn relocate<PreS, PostS>(
        core: &ElfModule<()>,
        rel_type: &ElfRelType,
        pltgot: &mut PltGotSection,
        scope: &[LoadedModule<()>],
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
