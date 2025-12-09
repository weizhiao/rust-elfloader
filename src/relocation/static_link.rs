use core::marker::PhantomData;

use crate::{
    CoreComponent, Result,
    arch::{ElfRelType, StaticRelocator},
    format::{Relocated, relocatable::ElfRelocatable},
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

impl ElfRelocatable {
    pub(crate) fn relocate_impl<'lib, 'iter, 'find, F>(
        mut self,
        scope: &[&'iter Relocated],
        pre_find: &'find F,
    ) -> Result<Relocated<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        'iter: 'lib,
        'find: 'lib,
    {
        for reloc in self.relocation.relocation.iter() {
            for rel in *reloc {
                StaticRelocator::relocate(&self.core, rel, &mut self.pltgot, scope, pre_find)?;
            }
        }
        (self.mprotect)()?;
        (self.init)(None, self.init_array);
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
        pltgot: &mut PltGotSection,
        scope: &[&Relocated],
        pre_find: &F,
    ) -> Result<()>
    where
        F: Fn(&str) -> Option<*const ()>;

    fn needs_got(_rel_type: u32) -> bool {
        false
    }

    fn needs_plt(_rel_type: u32) -> bool {
        false
    }
}
