use crate::{Loader, Result, format::Relocated, mmap::Mmap, object::ElfObject};

impl<M: Mmap> Loader<M> {
    pub fn load_relocatable(&mut self, mut object: impl ElfObject) -> Result<()> {
        let ehdr = self.buf.prepare_ehdr(&mut object).unwrap();
        self.load_rel(ehdr, object)?;
        Ok(())
    }
}

pub struct ElfRelocatable {}

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

struct RelocatedObject<'scope> {
    inner: Relocated<'scope>,
}

impl<'scope> RelocatedObject<'scope> {}
