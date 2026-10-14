//! Internal installation and relocation of lazy PLT entries.

use super::{LazyPlacement, LazyRuntime, LazySetup};
use crate::{
    ByteRepr, LazyBindingError, RelocationError, Result,
    elf::{ElfLayout, ElfRelEntry, ElfRelType, ElfWord},
    image::RawDynamic,
    memory::{ImageMemory, ImageMemoryExt, RegionAccess, VmAddr, VmOffset},
    relocation::RelocationArch,
    tls::TlsResolver,
};
use core::mem::size_of;

pub(crate) fn install_plt<Arch, D, R, Tls>(
    setup: LazySetup,
    image: &RawDynamic<D, Arch, R, Tls>,
) -> Result<()>
where
    Arch: RelocationArch,
    D: Send + Sync + 'static,
    R: RegionAccess,
    Tls: TlsResolver<Arch>,
{
    if image.relocation().pltrel().is_empty() {
        return Ok(());
    }

    let placement = Arch::LAZY_BINDING;
    let runtime = LazyRuntime::<Arch>::new(&image.inner.runtime);

    if let LazyPlacement::Slots { context, resolver } = placement {
        let values = setup.values();
        let word_size = size_of::<<Arch::Layout as ElfLayout>::Word>();
        let got_plt = image.got_plt().ok_or(RelocationError::LazyBinding(
            LazyBindingError::MissingGotPlt,
        ))?;
        let context_slot = got_plt + VmOffset::new(context * word_size);
        let resolver_slot = got_plt + VmOffset::new(resolver * word_size);
        let context = <Arch::Layout as ElfLayout>::Word::from_usize(values.context().get());
        let resolver = <Arch::Layout as ElfLayout>::Word::from_usize(values.resolver().get());
        unsafe {
            runtime.memory().write_value(context_slot, context)?;
            runtime.memory().write_value(resolver_slot, resolver)?;
        }
    }
    runtime.core().set_lazy(setup);
    Ok(())
}

pub(crate) fn relocate_jump_slot<Arch, Memory>(
    lazy: bool,
    memory: &Memory,
    base: VmAddr,
    rel: &ElfRelType<Arch>,
) -> Result<bool>
where
    Arch: RelocationArch,
    Memory: ImageMemory,
    <Arch::Layout as ElfLayout>::Word: ByteRepr,
{
    if !lazy {
        return Ok(false);
    }

    unsafe {
        memory.update_value(
            base + rel.r_offset(),
            |word: <Arch::Layout as ElfLayout>::Word| {
                <Arch::Layout as ElfLayout>::Word::from_usize(
                    (base + VmOffset::new(word.to_usize())).get(),
                )
            },
        )?
    };
    Ok(true)
}
