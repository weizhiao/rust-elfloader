//! Traits implemented by custom lazy PLT binders.

use super::{LazyRuntime, LazySetup};
use crate::{Result, relocation::RelocationArch, sync::Arc};

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
///     fn prepare(&self, _runtime: LazyRuntime<'_, Arch>) -> Result<Option<LazySetup>> {
///         Ok(Some(LazySetup::new(self.context, self.resolver)))
///     }
/// }
///
/// let relocator = Relocator::new().lazy_binder(RemoteLazyBinder {
///     context: VmAddr::new(0x1000),
///     resolver: VmAddr::new(0x2000),
/// });
/// ```
pub trait LazyBinder<Arch: RelocationArch>: Send + Sync + 'static {
    /// Prepares the lazy runtime, or returns `None` when this binder cannot
    /// support the target environment. Preparation errors are propagated;
    /// they do not trigger a fallback to eager binding.
    ///
    /// Binding policy is applied by the relocation flow. The runtime is borrowed
    /// only for setup. Stored raw context addresses do not keep the module alive;
    /// resolvers must ensure validity before using them.
    fn prepare(&self, runtime: LazyRuntime<'_, Arch>) -> Result<Option<LazySetup>>;
}

impl<Arch: RelocationArch> LazyBinder<Arch> for () {
    #[inline]
    fn prepare(&self, _runtime: LazyRuntime<'_, Arch>) -> Result<Option<LazySetup>> {
        Ok(None)
    }
}

impl<Arch, B> LazyBinder<Arch> for Arc<B>
where
    Arch: RelocationArch,
    B: LazyBinder<Arch> + ?Sized,
{
    #[inline]
    fn prepare(&self, runtime: LazyRuntime<'_, Arch>) -> Result<Option<LazySetup>> {
        (**self).prepare(runtime)
    }
}
