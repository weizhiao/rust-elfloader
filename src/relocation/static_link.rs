use core::marker::PhantomData;

use crate::{
    CoreComponent, Result,
    arch::{ElfRelType, StaticRelocator},
    format::{Relocated, relocatable::ElfRelocatable},
    segment::shdr::PltGotSection,
};
use alloc::{boxed::Box, vec::Vec};

pub(crate) struct StaticRelocation {
    relocation: Box<[(&'static [ElfRelType], usize)]>,
}

impl StaticRelocation {
    pub(crate) fn new(relocation: Vec<(&'static [ElfRelType], usize)>) -> Self {
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
        for (reloc, target_base) in self.relocation.relocation.iter() {
            for rel in *reloc {
                StaticRelocator::relocate(
                    &self.core,
                    rel,
                    &mut self.pltgot,
                    *target_base,
                    scope,
                    pre_find,
                    self.lazy_bind,
                )?;
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
        pltgot: &mut PltGotSection,
        target_base: usize,
        scope: &[&Relocated],
        pre_find: &F,
        lazy: bool,
    ) -> Result<()>
    where
        F: Fn(&str) -> Option<*const ()>;
}
