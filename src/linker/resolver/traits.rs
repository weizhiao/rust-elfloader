use super::{super::ModuleKey, ResolveRequest};
use crate::{
    Result, arch::NativeArch, image::ModuleHandle, input::ElfReader, relocation::RelocationArch,
};
use alloc::{boxed::Box, vec::Vec};

pub(crate) enum ResolvedKind<'cfg, Arch: RelocationArch = NativeArch> {
    /// Provides an ELF reader for the requested key.
    Load(Box<dyn ElfReader + 'cfg>),
    /// Provides a module candidate and its dependencies.
    ///
    /// The underlying allocation may already be shared elsewhere. A visible
    /// module with the requested key is reused; otherwise this module is
    /// published and initialized with the transaction.
    Module {
        /// Module exposed for symbol lookup.
        module: ModuleHandle<Arch>,
        /// Dependencies resolved as part of this graph fragment.
        deps: Vec<ResolvedDependency<'cfg, Arch>>,
    },
}

/// A key-resolution result chosen by caller policy.
pub struct ResolvedKey<'cfg, Arch: RelocationArch = NativeArch> {
    kind: ResolvedKind<'cfg, Arch>,
    pinned: bool,
    aliases: Vec<ModuleKey>,
}

/// One named dependency in a pre-resolved module graph.
///
/// The key belongs to the dependency edge, while [`ResolvedKey`] describes the
/// module selected for it.
pub struct ResolvedDependency<'cfg, Arch: RelocationArch = NativeArch> {
    pub(crate) key: ModuleKey,
    pub(crate) resolved: ResolvedKey<'cfg, Arch>,
}

impl<'cfg, Arch: RelocationArch> ResolvedDependency<'cfg, Arch> {
    /// Creates a named dependency from a resolver result.
    #[inline]
    pub fn new(key: impl Into<ModuleKey>, resolved: ResolvedKey<'cfg, Arch>) -> Self {
        Self {
            key: key.into(),
            resolved,
        }
    }
}

impl<'cfg, Arch: RelocationArch> ResolvedKey<'cfg, Arch> {
    /// Creates a result that loads a new module from the provided reader.
    #[inline]
    pub fn load(reader: impl ElfReader + 'cfg) -> Self {
        Self::load_boxed(Box::new(reader))
    }

    #[inline]
    pub(super) fn load_boxed(reader: Box<dyn ElfReader + 'cfg>) -> Self {
        Self {
            kind: ResolvedKind::Load(reader),
            pinned: false,
            aliases: Vec::new(),
        }
    }

    /// Creates a result backed by a module not yet committed to this context.
    #[inline]
    pub fn module(
        module: impl Into<ModuleHandle<Arch>>,
        deps: impl Into<Vec<ResolvedDependency<'cfg, Arch>>>,
    ) -> Self {
        Self {
            kind: ResolvedKind::Module {
                module: module.into(),
                deps: deps.into(),
            },
            pinned: false,
            aliases: Vec::new(),
        }
    }

    /// Adds alternative request keys for the selected module.
    ///
    /// Aliases become visible within the pending load before its dependencies
    /// are resolved. They are committed with the selected module and discarded
    /// if preparation or relocation fails. Published aliases follow the module's
    /// lifetime; the existing context's key precedence is preserved.
    pub fn with_aliases<K: Into<ModuleKey>>(
        mut self,
        aliases: impl IntoIterator<Item = K>,
    ) -> Self {
        self.aliases.extend(aliases.into_iter().map(Into::into));
        self
    }

    /// Keeps the resolved module committed for the lifetime of its context.
    ///
    /// The pin is applied transactionally when the load is published. It is
    /// removed if publication is rolled back.
    #[inline]
    pub const fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }

    #[inline]
    pub(crate) fn into_parts(self) -> (ResolvedKind<'cfg, Arch>, bool, Vec<ModuleKey>) {
        (self.kind, self.pinned, self.aliases)
    }
}

/// Key-resolution policy used by [`super::super::Linker`].
pub trait KeyResolver<Arch: RelocationArch = NativeArch> {
    /// Root input owned by one linker load operation.
    ///
    /// It may carry non-cloneable resources such as an already-open file.
    type Root;

    /// Maps a root input to the key used for an existing-module lookup.
    fn root_key<'a>(&self, root: &'a Self::Root) -> &'a [u8];

    /// Normalizes a request key for existing-module lookup.
    ///
    /// Policies that replace several paths with one virtual module can return
    /// its canonical name. The original root or dependency is still passed to
    /// [`Self::resolve`], so filesystem lookup retains the requested path. The
    /// resolver result must register this canonical name as an alias.
    #[inline]
    fn normalize_key<'a>(&'a self, key: &'a [u8]) -> &'a [u8] {
        key
    }

    /// Resolves a root input or one `DT_NEEDED` dependency.
    fn resolve<'cfg>(&self, req: ResolveRequest<'_, Self::Root>)
    -> Result<ResolvedKey<'cfg, Arch>>;
}
