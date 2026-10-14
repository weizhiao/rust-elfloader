//! Traits implemented by custom lazy PLT binders.

use super::defs::{LazyPlacement, LazyRuntime, LazySetup};
use crate::{
    ByteRepr, LazyBindingError, RelocationError, Result,
    elf::{ElfLayout, ElfRelEntry, ElfRelType, ElfWord},
    image::RawDynamic,
    memory::{ImageMemory, ImageMemoryExt, RegionAccess, VmAddr, VmOffset},
    relocation::RelocationArch,
    sync::Arc,
    tls::TlsResolver,
};
use core::mem::size_of;

/// Marker trait for raw image types that support lazy-binding fixup hooks.
pub trait SupportLazy {}

impl SupportLazy for () {}

/// Supplies lazy PLT runtime entries for a mapped image.
///
/// Implement this trait when the default same-process lazy binder is not the
/// right execution model, for example when the loaded image runs in a remote VM
/// or on a different architecture. The binder receives a [`LazyRuntime`] handle
/// for the mapped image and returns the target-visible values and retained state
/// needed by the architecture's lazy-binding placement.
///
/// # Example
///
/// ```
/// use elf_loader::{
///     Relocator, Result,
///     lazy::{LazyBinder, LazyRuntime, LazySetup},
///     memory::VmAddr,
///     relocation::RelocationArch,
/// };
///
/// #[derive(Clone, Copy)]
/// struct RemoteLazyBinder {
///     context: VmAddr,
///     resolver: VmAddr,
/// }
///
/// impl<Arch: RelocationArch> LazyBinder<Arch> for RemoteLazyBinder {
///     fn prepare_slots(&self, _runtime: LazyRuntime<'_, Arch>) -> Result<LazySetup> {
///         Ok(LazySetup::new(self.context, self.resolver))
///     }
/// }
///
/// let relocator = Relocator::new().lazy_binder(RemoteLazyBinder {
///     context: VmAddr::new(0x1000),
///     resolver: VmAddr::new(0x2000),
/// });
/// ```
pub trait LazyBinder<Arch: RelocationArch>: Send + Sync + 'static {
    /// Reports whether this binder can provide a lazy resolver for this architecture.
    /// Binding policy is applied by the relocation flow, not by the binder.
    #[inline]
    fn supports_lazy(&self) -> bool {
        true
    }

    /// Builds the values installed into this image's lazy binding slots.
    /// The runtime is borrowed only for setup. Stored raw context addresses do
    /// not keep the module alive; resolvers must ensure validity before using them.
    fn prepare_slots(&self, runtime: LazyRuntime<'_, Arch>) -> Result<LazySetup>;
}

impl<Arch: RelocationArch> LazyBinder<Arch> for () {
    #[inline]
    fn supports_lazy(&self) -> bool {
        false
    }

    #[inline]
    fn prepare_slots(&self, _runtime: LazyRuntime<'_, Arch>) -> Result<LazySetup> {
        Err(RelocationError::LazyBinding(LazyBindingError::MissingBinder).into())
    }
}

impl<Arch, B> LazyBinder<Arch> for Arc<B>
where
    Arch: RelocationArch,
    B: LazyBinder<Arch> + ?Sized,
{
    #[inline]
    fn supports_lazy(&self) -> bool {
        (**self).supports_lazy()
    }

    #[inline]
    fn prepare_slots(&self, runtime: LazyRuntime<'_, Arch>) -> Result<LazySetup> {
        (**self).prepare_slots(runtime)
    }
}

pub(crate) fn prepare_plt<Arch, Binder, D, R, Tls>(
    binder: &Binder,
    lazy: bool,
    image: &RawDynamic<D, Arch, R, Tls>,
) -> Result<()>
where
    Arch: RelocationArch,
    Binder: LazyBinder<Arch> + ?Sized,
    D: Send + Sync + 'static,
    R: RegionAccess,
    Tls: TlsResolver<Arch>,
{
    if !lazy || image.relocation().pltrel().is_empty() {
        return Ok(());
    }

    let placement = Arch::LAZY_BINDING;
    if placement == LazyPlacement::Unsupported {
        return Err(RelocationError::LazyBinding(LazyBindingError::Unsupported).into());
    }

    let runtime = LazyRuntime::<Arch>::new(&image.inner.runtime);
    let setup = binder.prepare_slots(runtime)?;

    if let LazyPlacement::Slots(slots) = placement {
        let values = setup.values();
        let word_size = size_of::<<Arch::Layout as ElfLayout>::Word>();
        let got_plt = image.got_plt().ok_or(RelocationError::LazyBinding(
            LazyBindingError::MissingGotPlt,
        ))?;
        let context_slot = got_plt + VmOffset::new(slots.context() * word_size);
        let resolver_slot = got_plt + VmOffset::new(slots.resolver() * word_size);
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
