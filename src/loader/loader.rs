use super::run::LoaderRun;
use crate::{
    MmapError, Result,
    arch::NativeArch,
    const_builder::NoDrop,
    os::{DefaultMmap, Mmap, PageSize},
    relocation::RelocationArch,
    runtime::Domain,
    tls::TlsResolver,
};
#[cfg(feature = "object")]
use crate::{object::SectionGroups, sync::Arc};
use core::{marker::PhantomData, mem::MaybeUninit, ptr};

/// Reusable configuration for reading and mapping ELF images.
///
/// `Loader` maps ELF objects from files or memory and produces raw image types such as
/// [`crate::image::RawElf`], [`crate::image::RawDynamic`], [`crate::image::RawDylib`],
/// and [`crate::image::RawExec`].
/// Those raw images can then be relocated with [`crate::Relocator`].
///
/// A loader contains only stable configuration: target architecture, mapping
/// backend and user-data type. The runtime domain, scratch state
/// and observers live in [`LoaderRun`], so a configured loader can be reused
/// across many inputs.
///
/// Start with [`Loader::new`], call configuration methods such as
/// [`for_arch`](Self::for_arch), then select the runtime with [`Loader::run`].
/// Native loaders also provide `load_*` convenience methods. Attach per-run observers with
/// [`LoaderRun::with_observer`]. Use
/// [`Relocator`](crate::Relocator) for explicit symbol scopes and final
/// relocation, or [`Linker`](crate::Linker) when `DT_NEEDED` dependencies should
/// be resolved automatically.
///
/// # Examples
///
/// ```no_run
/// use elf_loader::{Loader, Relocator};
///
/// let config = Loader::new();
/// use elf_loader::{AddressSpaceId, Domain};
/// let domain = Domain::native(AddressSpaceId::PROCESS);
/// let mut loader = config.run(domain);
/// let raw = loader.load_dylib("path/to/liba.so").unwrap();
/// let lib = Relocator::new().run(raw).relocate().unwrap();
/// ```
pub struct Loader<D: Send + Sync + 'static = (), Arch = NativeArch, M = DefaultMmap>
where
    Arch: RelocationArch,
    M: Mmap,
{
    mapper: M,
    page_size: Option<PageSize>,
    force_static_tls: bool,
    _marker: PhantomData<fn() -> (D, Arch)>,
}

impl<D: Send + Sync + 'static, Arch, M> Copy for Loader<D, Arch, M>
where
    D: Send + Sync + 'static,
    Arch: RelocationArch,
    M: Mmap + Copy,
{
}

struct LoaderFields<M> {
    mapper: NoDrop<M>,
    page_size: Option<PageSize>,
    force_static_tls: bool,
}

impl<M> LoaderFields<M> {
    #[inline]
    const fn into_loader<D, Arch>(self) -> Loader<D, Arch, M>
    where
        D: Send + Sync + 'static,
        Arch: RelocationArch,
        M: Mmap,
    {
        let Self {
            mapper,
            page_size,
            force_static_tls,
        } = self;
        Loader {
            mapper: mapper.into_inner(),
            page_size,
            force_static_tls,
            _marker: PhantomData,
        }
    }

    #[inline]
    const fn with_mapper<D, Arch, NewM>(self, mapper: NewM) -> Loader<D, Arch, NewM>
    where
        D: Send + Sync + 'static,
        Arch: RelocationArch,
        NewM: Mmap,
        M: Copy,
    {
        let Self {
            page_size,
            force_static_tls,
            ..
        } = self;
        Loader {
            mapper,
            page_size,
            force_static_tls,
            _marker: PhantomData,
        }
    }
}

impl<D: Send + Sync + 'static, Arch, M> Clone for Loader<D, Arch, M>
where
    D: Send + Sync + 'static,
    Arch: RelocationArch,
    M: Mmap + Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            mapper: self.mapper.clone(),

            page_size: self.page_size,
            force_static_tls: self.force_static_tls,
            _marker: PhantomData,
        }
    }
}

impl Loader<(), NativeArch, DefaultMmap> {
    /// Creates a new [`Loader`] with the default mmap backend, no observer, no
    /// custom user data, no TLS resolver, and the host target architecture
    /// ([`NativeArch`]).
    ///
    /// To target a different ELF architecture (e.g. load an x86-64 shared
    /// object on a RISC-V host), switch the target architecture with
    /// [`for_arch::<NewArch>()`](Self::for_arch); the `e_machine` gate
    /// then validates against `NewArch::MACHINE` automatically.
    #[inline]
    pub const fn new() -> Self {
        Self {
            mapper: DefaultMmap::new(),

            page_size: None,
            force_static_tls: false,
            _marker: PhantomData,
        }
    }
}

impl Default for Loader<(), NativeArch, DefaultMmap> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<D: Send + Sync + 'static, Arch, M> Loader<D, Arch, M>
where
    D: Send + Sync + 'static,
    Arch: RelocationArch,
    M: Mmap,
{
    #[inline]
    const fn into_fields(self) -> LoaderFields<M> {
        let this = MaybeUninit::new(self);
        let this = this.as_ptr();
        unsafe {
            LoaderFields {
                mapper: NoDrop::read(ptr::addr_of!((*this).mapper)),

                page_size: ptr::read(ptr::addr_of!((*this).page_size)),
                force_static_tls: ptr::read(ptr::addr_of!((*this).force_static_tls)),
            }
        }
    }
}

impl<D: Send + Sync + 'static, Arch, M> Loader<D, Arch, M>
where
    D: Send + Sync + 'static,
    Arch: RelocationArch,
    M: Mmap,
{
    #[inline]
    pub(crate) const fn mapper(&self) -> &M {
        &self.mapper
    }

    #[inline]
    pub(crate) const fn force_static_tls(&self) -> bool {
        self.force_static_tls
    }

    /// Starts a loader run in a fixed runtime domain with fresh scratch space.
    /// The mapping backend must target the domain's address space.
    #[inline]
    pub fn run<Tls: TlsResolver<Arch>>(
        &self,
        domain: Domain<Arch, Tls>,
    ) -> LoaderRun<'_, (), D, Tls, Arch, M> {
        LoaderRun {
            loader: self,
            domain,
            observer: (),
            buf: super::ElfBuf::new(),
            search_paths: None,
            #[cfg(feature = "object")]
            object_groups: Arc::new(SectionGroups::default()),
        }
    }

    #[inline]
    pub(crate) fn page_size(&self) -> Result<PageSize> {
        let required = self.mapper.page_size();
        let page_size = self.page_size.unwrap_or(required);
        if page_size.bytes() < required.bytes()
            || !page_size.bytes().is_multiple_of(required.bytes())
        {
            return Err(MmapError::InvalidPageSize {
                configured: page_size.bytes(),
                required: required.bytes(),
            }
            .into());
        }

        Ok(page_size)
    }

    /// Consumes the current loader and returns a new one with the specified
    /// dynamic-image user data type.
    ///
    /// Dynamic images are created with `NewD::default()`. To fill or adjust
    /// that data after dynamic metadata has been parsed, implement
    /// [`LoadObserver::on_after_dynamic_load`](crate::observer::LoadObserver::on_after_dynamic_load)
    /// on the configured load observer.
    pub const fn with_data<NewD>(self) -> Loader<NewD, Arch, M>
    where
        NewD: Default + Send + Sync + 'static,
    {
        self.into_fields().into_loader()
    }

    /// Overrides the base page size used for segment layout decisions.
    ///
    /// By default, the loader uses [`Mmap::page_size`]. An override is useful
    /// for special runtimes and tests, but it must remain compatible with the
    /// mapping backend and with every loaded ELF's `PT_LOAD` alignment.
    pub const fn with_page_size(mut self, page_size: PageSize) -> Self {
        self.page_size = Some(page_size);
        self
    }

    /// Sets whether to force static TLS for all loaded modules.
    pub const fn with_static_tls(mut self, enabled: bool) -> Self {
        self.force_static_tls = enabled;
        self
    }
}

/// Cross-architecture builder step.
///
/// Switching the target architecture is only meaningful while the loader has
/// not yet been bound to a user-data type. The builder therefore exposes
/// [`Loader::for_arch`] only on loaders whose `D` is still `()`. Callers should
/// pick the target architecture first and attach the user-data type afterwards:
///
/// ```no_run
/// use elf_loader::Loader;
/// use elf_loader::arch::x86_64::X86_64Arch;
///
/// let _loader = Loader::new()
///     .for_arch::<X86_64Arch>()
///     .with_data::<()>();
/// ```
impl<Arch, M> Loader<(), Arch, M>
where
    Arch: RelocationArch,
    M: Mmap,
{
    /// Returns a new loader with a custom `Mmap` backend.
    pub const fn with_mmap<NewMmap>(self, mapper: NewMmap) -> Loader<(), Arch, NewMmap>
    where
        NewMmap: Mmap,
        M: Copy,
    {
        self.into_fields().with_mapper(mapper)
    }

    /// Consumes the current loader and returns a new one whose target
    /// architecture is `NewArch` instead of the previous `Arch`.
    ///
    /// This is the primary entry point for cross-architecture loading. Picking
    /// a non-host architecture (e.g.
    /// [`X86_64Arch`](crate::arch::x86_64::X86_64Arch)) makes
    /// every subsequent `load_*` call validate the ELF `e_machine` against
    /// `NewArch::MACHINE` instead of the host's, and stamps the resulting
    /// raw images with `NewArch` so [`Relocator::run`](crate::Relocator::run)
    /// and [`RelocatorRun::relocate`](crate::RelocatorRun::relocate) use the
    /// matching relocation numbering.
    ///
    /// Select a matching runtime executor through [`Domain`] and [`Loader::run`]
    /// before loading a non-native image. The executor handles its IFUNC,
    /// TLSDESC and lifecycle calls.
    ///
    /// # Builder ordering
    ///
    /// `for_arch` is only available before
    /// [`with_data`](Loader::with_data) has been called; instead, switch `Arch`
    /// first and then attach the user-data type once the target architecture is
    /// fixed.
    ///
    /// [`RelocatorRun::relocate`]: crate::RelocatorRun::relocate
    pub const fn for_arch<NewArch>(self) -> Loader<(), NewArch, M>
    where
        NewArch: RelocationArch,
    {
        self.into_fields().into_loader()
    }
}

#[cfg(test)]
mod tests {
    use super::Loader;
    use crate::runtime::{AddressSpaceId, Domain};

    #[test]
    fn run_preserves_runtime_domain() {
        let domain = Domain::native(AddressSpaceId::new());
        let loader = Loader::new().with_data::<()>();
        let run = loader.run(domain.clone());

        assert_eq!(run.domain.id(), domain.id());
    }
}
