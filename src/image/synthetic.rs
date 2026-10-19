use super::{Module, ModuleHandle, ModuleState, SymbolExports, SymbolLookup, SymbolName};
use crate::{
    Result,
    arch::NativeArch,
    custom_error,
    elf::{ElfLayout, ElfSectionIndex, ElfSymbol, ElfSymbolBind, ElfSymbolType},
    input::ModuleSourceId,
    memory::{ImageMemory, VmAddr, VmOffset},
    relocation::RelocationArch,
    runtime::{CodeContext, CodeExecutor, Domain},
    sync::{Arc, arc_unsize},
    tls::{TlsModule, TlsRegistration, TlsResolver},
};
use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::{ffi::CStr, ptr::NonNull};

/// A symbol exported by a [`SyntheticModule`], such as a host callback or variable.
/// Use a C string literal or [`SymbolName`] for its name.
#[derive(Clone, Debug)]
pub struct SyntheticSymbol {
    name: SymbolName,
    version: Option<SymbolVersion>,
    value: usize,
    size: usize,
    bind: ElfSymbolBind,
    symbol_type: ElfSymbolType,
    other: u8,
    section_index: ElfSectionIndex,
}

/// GNU symbol-version metadata for a [`SyntheticSymbol`].
#[derive(Clone, Debug)]
pub struct SymbolVersion {
    name: Vec<u8>,
    default: bool,
}

impl SymbolVersion {
    /// Creates symbol-version metadata.
    ///
    /// A default version corresponds to GNU `name@@version`; a non-default
    /// version corresponds to `name@version`.
    #[inline]
    pub fn new(name: impl AsRef<[u8]>, default: bool) -> Self {
        Self {
            name: name.as_ref().to_vec(),
            default,
        }
    }

    /// Returns the version name.
    #[inline]
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    /// Returns whether this version satisfies an unversioned lookup.
    #[inline]
    pub const fn is_default(&self) -> bool {
        self.default
    }
}

impl SyntheticSymbol {
    /// Creates a function symbol backed by an absolute runtime address.
    #[inline]
    pub fn function(name: impl Into<SymbolName>, value: *const ()) -> Self {
        Self::from_fields(
            name,
            value as usize,
            0,
            ElfSymbolBind::GLOBAL,
            ElfSymbolType::FUNC,
            0,
            ElfSectionIndex::ABS,
            None,
        )
    }

    /// Creates an object symbol backed by an absolute runtime address.
    #[inline]
    pub fn object(name: impl Into<SymbolName>, value: *const (), size: usize) -> Self {
        Self::from_fields(
            name,
            value as usize,
            size,
            ElfSymbolBind::GLOBAL,
            ElfSymbolType::OBJECT,
            0,
            ElfSectionIndex::ABS,
            None,
        )
    }

    /// Creates a TLS symbol whose value is an offset in this module's TLS block.
    #[inline]
    pub fn tls(name: impl Into<SymbolName>, offset: usize, size: usize) -> Self {
        Self::from_fields(
            name,
            offset,
            size,
            ElfSymbolBind::GLOBAL,
            ElfSymbolType::TLS,
            0,
            ElfSectionIndex::new(1),
            None,
        )
    }

    /// Creates a symbol with explicit ELF fields and optional GNU version metadata.
    /// The module assigns `st_name` when the symbol is inserted.
    #[inline]
    pub fn from_fields(
        name: impl Into<SymbolName>,
        value: usize,
        size: usize,
        bind: ElfSymbolBind,
        symbol_type: ElfSymbolType,
        other: u8,
        section_index: ElfSectionIndex,
        version: Option<SymbolVersion>,
    ) -> Self {
        Self {
            name: name.into(),
            version,
            value,
            size,
            bind,
            symbol_type,
            other,
            section_index,
        }
    }

    /// Creates a synthetic symbol from an ELF symbol-table entry.
    ///
    /// `name` must be supplied separately because [`ElfSymbol::st_name`] is an
    /// index into the source image's string table. GNU version metadata likewise
    /// lives outside the ELF symbol-table entry.
    #[inline]
    pub fn from_elf<L: ElfLayout>(
        name: impl Into<SymbolName>,
        symbol: &ElfSymbol<L>,
        version: Option<SymbolVersion>,
    ) -> Self {
        Self::from_fields(
            name,
            symbol.st_value(),
            symbol.st_size(),
            symbol.bind(),
            symbol.symbol_type(),
            symbol.st_other(),
            symbol.st_shndx(),
            version,
        )
    }

    /// Exports this symbol with `version`.
    ///
    /// A default definition corresponds to GNU `name@@version` and may satisfy
    /// an unversioned lookup. A non-default definition corresponds to
    /// `name@version` and is available only to an exact versioned lookup.
    #[inline]
    pub fn with_version(mut self, version: impl AsRef<[u8]>, default: bool) -> Self {
        self.version = Some(SymbolVersion::new(version, default));
        self
    }

    /// Sets the ELF symbol binding used by the synthetic symbol.
    #[inline]
    pub fn with_bind(mut self, bind: ElfSymbolBind) -> Self {
        self.bind = bind;
        self
    }

    /// Sets the ELF symbol size.
    #[inline]
    pub fn with_size(mut self, size: usize) -> Self {
        self.size = size;
        self
    }

    /// Sets the ELF `st_other` value.
    #[inline]
    pub fn with_other(mut self, other: u8) -> Self {
        self.other = other;
        self
    }

    /// Sets the ELF section index used by the synthetic symbol.
    ///
    /// Function and object symbols default to [`ElfSectionIndex::ABS`] because
    /// they normally carry an already-resolved runtime address. TLS and
    /// module-relative symbols can override this with their real section index.
    #[inline]
    pub fn with_section(mut self, section_index: ElfSectionIndex) -> Self {
        self.section_index = section_index;
        self
    }
}

#[derive(Clone, Copy)]
struct UnmappedImageMemory {
    base: VmAddr,
}

impl Default for UnmappedImageMemory {
    #[inline]
    fn default() -> Self {
        Self {
            base: VmAddr::null(),
        }
    }
}

impl ImageMemory for UnmappedImageMemory {
    #[inline]
    fn base(&self) -> VmAddr {
        self.base
    }

    #[inline]
    fn range_at(&self, _addr: VmAddr) -> Option<core::ops::Range<VmAddr>> {
        None
    }

    fn visit_ranges(&self, _visit: &mut dyn FnMut(core::ops::Range<VmAddr>)) {}

    #[inline]
    fn host_ptr(&self, _addr: VmAddr) -> Option<NonNull<u8>> {
        None
    }

    #[inline]
    fn host_ptr_range(&self, _addr: VmAddr, _len: usize) -> Option<NonNull<u8>> {
        None
    }

    #[inline]
    fn read_bytes(&self, _addr: VmAddr, dst: &mut [u8]) -> Result<()> {
        if dst.is_empty() {
            return Ok(());
        }

        Err(custom_error(
            "synthetic modules do not expose readable image bytes",
        ))
    }

    #[inline]
    fn write_bytes(&self, _addr: VmAddr, src: &[u8]) -> Result<()> {
        if src.is_empty() {
            return Ok(());
        }

        Err(custom_error(
            "synthetic modules do not expose writable image bytes",
        ))
    }
}

/// A module of caller-provided symbols, usable in a [`ModuleScope`](crate::ModuleScope).
/// Owns its symbol metadata without loading an ELF image.
pub struct SyntheticModule<Arch: RelocationArch = NativeArch, D = (), R = ()> {
    state: ModuleState,
    name: String,
    memory: Arc<dyn ImageMemory>,
    tls: Option<TlsRegistration>,
    executor: Arc<dyn CodeExecutor<Arch>>,
    resolve_hook: R,
    user_data: D,
    names: Vec<SymbolName>,
    symbols: Vec<ElfSymbol<Arch::Layout>>,
    index: BTreeMap<SymbolName, SymbolIndex>,
}

/// Runtime callback invoked when a synthetic symbol is resolved.
pub trait ResolveHook<Arch: RelocationArch>: Send + Sync {
    /// Prepares runtime state associated with `symbol` and its stable address.
    fn resolve(&self, symbol: &ElfSymbol<Arch::Layout>, address: VmAddr) -> Result<()>;
}

impl<Arch: RelocationArch> ResolveHook<Arch> for () {
    #[inline]
    fn resolve(&self, _symbol: &ElfSymbol<Arch::Layout>, _address: VmAddr) -> Result<()> {
        Ok(())
    }
}

impl<Arch, F> ResolveHook<Arch> for F
where
    Arch: RelocationArch,
    F: Fn(&ElfSymbol<Arch::Layout>, VmAddr) -> Result<()> + Send + Sync,
{
    #[inline]
    fn resolve(&self, symbol: &ElfSymbol<Arch::Layout>, address: VmAddr) -> Result<()> {
        self(symbol, address)
    }
}

#[derive(Clone, Default)]
struct SymbolIndex {
    default: Option<usize>,
    versions: Vec<(SymbolVersion, usize)>,
}

impl<Arch: RelocationArch> SyntheticModule<Arch> {
    /// Creates a module from an ordered list of synthetic symbols in `domain`.
    pub fn new<I, Tls: TlsResolver<Arch>>(
        domain: &Domain<Arch, Tls>,
        name: impl Into<String>,
        symbols: I,
    ) -> Self
    where
        I: IntoIterator<Item = SyntheticSymbol>,
    {
        let mut module = Self::empty(domain, name);
        for symbol in symbols {
            let _ = module.insert(symbol);
        }
        module
    }

    /// Creates an empty synthetic module bound to an immutable runtime domain.
    pub fn empty<Tls: TlsResolver<Arch>>(
        domain: &Domain<Arch, Tls>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            state: ModuleState::new(ModuleSourceId::fresh(), domain.id()),
            name: name.into(),
            memory: arc_unsize!(
                Arc::new(UnmappedImageMemory::default()) => dyn ImageMemory
            ),
            tls: None,
            executor: domain.shared_executor(),
            resolve_hook: (),
            user_data: (),
            names: Vec::new(),
            symbols: Vec::new(),
            index: BTreeMap::new(),
        }
    }
}

impl<Arch: RelocationArch, D, R> SyntheticModule<Arch, D, R> {
    /// Replaces the user data associated with this synthetic module.
    #[inline]
    pub fn with_data<NewD>(self, user_data: NewD) -> SyntheticModule<Arch, NewD, R> {
        SyntheticModule {
            state: self.state,
            name: self.name,
            memory: self.memory,
            tls: self.tls,
            executor: self.executor,
            resolve_hook: self.resolve_hook,
            user_data,
            names: self.names,
            symbols: self.symbols,
            index: self.index,
        }
    }

    /// Returns immutable user data for this synthetic module.
    #[inline]
    pub const fn user_data(&self) -> &D {
        &self.user_data
    }

    /// Returns mutable user data for this synthetic module.
    #[inline]
    pub fn user_data_mut(&mut self) -> &mut D {
        &mut self.user_data
    }

    /// Uses a real image-memory backend for this synthetic module.
    ///
    /// This is required when synthetic symbols may be used as byte sources, such
    /// as for COPY relocations against synthetic object symbols.
    #[inline]
    pub fn with_memory<M>(mut self, memory: M) -> Self
    where
        M: ImageMemory + 'static,
    {
        self.memory = arc_unsize!(Arc::new(memory) => dyn ImageMemory);
        self
    }

    /// Transfers a TLS registration into this module. Its provider and runtime
    /// remain alive until the module is released. Returns an error if the entry
    /// belongs to a different TLS runtime than this domain.
    #[inline]
    pub fn with_tls(mut self, tls: TlsRegistration) -> Result<Self> {
        tls.module().check_domain(self.state.domain_id())?;
        self.tls = Some(tls);
        Ok(self)
    }

    /// Runs a callback when a synthetic symbol is resolved.
    ///
    /// The callback receives the symbol's stable address and may prepare lazy
    /// runtime state before that address is returned.
    #[inline]
    pub fn with_resolve_hook<F>(self, hook: F) -> SyntheticModule<Arch, D, F>
    where
        F: Fn(&ElfSymbol<Arch::Layout>, VmAddr) -> Result<()> + Send + Sync,
    {
        SyntheticModule {
            state: self.state,
            name: self.name,
            memory: self.memory,
            tls: self.tls,
            executor: self.executor,
            resolve_hook: hook,
            user_data: self.user_data,
            names: self.names,
            symbols: self.symbols,
            index: self.index,
        }
    }

    /// Inserts one symbol, returning the definition it replaced.
    pub fn insert(&mut self, symbol: SyntheticSymbol) -> Option<SyntheticSymbol> {
        let name = match self.index.get_key_value(symbol.name.as_bytes()) {
            Some((name, _)) => name.clone(),
            None => symbol.name,
        };
        let version = symbol.version;
        let entry = self.index.entry(name.clone()).or_default();
        let existing = match version.as_ref() {
            Some(version) => entry.versions.iter().find_map(|(entry, index)| {
                (entry.name == version.name).then(|| (*index, Some(entry.clone())))
            }),
            None => entry.default.map(|index| {
                let version = entry.versions.iter().find_map(|(version, version_index)| {
                    (*version_index == index && version.default).then(|| version.clone())
                });
                (index, version)
            }),
        };

        let (idx, previous) = if let Some((idx, previous_version)) = existing {
            let elf_symbol = ElfSymbol::synthetic(
                idx,
                symbol.value,
                symbol.size,
                symbol.bind,
                symbol.symbol_type,
                symbol.other,
                symbol.section_index,
            );
            let previous_symbol = core::mem::replace(&mut self.symbols[idx], elf_symbol);
            let previous = SyntheticSymbol::from_elf(
                self.names[idx].clone(),
                &previous_symbol,
                previous_version,
            );
            (idx, Some(previous))
        } else {
            let idx = self.symbols.len();
            let elf_symbol = ElfSymbol::synthetic(
                idx,
                symbol.value,
                symbol.size,
                symbol.bind,
                symbol.symbol_type,
                symbol.other,
                symbol.section_index,
            );
            self.names.push(name);
            self.symbols.push(elf_symbol);
            (idx, None)
        };

        match version {
            Some(version) => {
                let was_default = entry
                    .versions
                    .iter()
                    .position(|(entry, _)| entry.name == version.name)
                    .map(|position| entry.versions.remove(position).0.default)
                    .unwrap_or(false);
                if version.default {
                    for (entry, _) in &mut entry.versions {
                        entry.default = false;
                    }
                    entry.default = Some(idx);
                } else if was_default && entry.default == Some(idx) {
                    entry.default = None;
                }
                entry.versions.push((version, idx));
            }
            None => {
                if let Some(position) = entry
                    .versions
                    .iter()
                    .position(|(version, version_index)| *version_index == idx && version.default)
                {
                    entry.versions.remove(position);
                }
                entry.default = Some(idx);
            }
        }

        previous
    }

    /// Returns whether this module exports a synthetic symbol with `name`.
    #[inline]
    pub fn contains(&self, name: impl AsRef<[u8]>) -> bool {
        self.index.contains_key(name.as_ref())
    }

    /// Returns the number of synthetic symbols.
    #[inline]
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    /// Returns whether this module contains no symbols.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

impl<Arch, D, R> From<SyntheticModule<Arch, D, R>> for ModuleHandle<Arch>
where
    Arch: RelocationArch,
    D: Send + Sync + 'static,
    R: ResolveHook<Arch> + 'static,
{
    #[inline]
    fn from(module: SyntheticModule<Arch, D, R>) -> Self {
        Self::new(module)
    }
}

impl<Arch, D, R> Module<Arch> for SyntheticModule<Arch, D, R>
where
    Arch: RelocationArch,
    D: Send + Sync + 'static,
    R: ResolveHook<Arch> + 'static,
{
    #[inline]
    fn name(&self) -> &[u8] {
        self.name.as_bytes()
    }

    #[inline]
    fn exports(&self) -> &dyn SymbolExports<Arch::Layout> {
        self
    }

    #[inline]
    fn memory(&self) -> &dyn ImageMemory {
        &*self.memory
    }

    fn resolve_symbol(&self, symbol: &ElfSymbol<Arch::Layout>) -> Result<VmAddr> {
        match symbol.symbol_type() {
            ElfSymbolType::TLS => {
                return self
                    .tls
                    .as_ref()
                    .ok_or(crate::TlsError::TemplateUnavailable)?
                    .module()
                    .resolve(
                        self.executor.as_ref(),
                        CodeContext::new(self.name.as_bytes(), self.memory.as_ref()),
                        symbol.st_value(),
                    );
            }
            ElfSymbolType::GNU_IFUNC => {
                return Err(custom_error(
                    "synthetic module cannot execute IFUNC resolvers",
                ));
            }
            _ => {}
        }

        let address = if symbol.st_shndx().is_abs() {
            VmAddr::new(symbol.st_value())
        } else {
            self.memory.base() + VmOffset::new(symbol.st_value())
        };
        self.resolve_hook.resolve(symbol, address)?;
        Ok(address)
    }

    #[inline]
    fn tls(&self) -> Option<&TlsModule> {
        self.tls.as_ref().map(TlsRegistration::module)
    }

    #[inline]
    fn state(&self) -> &ModuleState {
        &self.state
    }
}

impl<Arch, D, R> SymbolExports<Arch::Layout> for SyntheticModule<Arch, D, R>
where
    Arch: RelocationArch,
    D: Send + Sync,
    R: Send + Sync,
{
    #[inline]
    fn for_each(&self, visitor: &mut dyn FnMut(&ElfSymbol<Arch::Layout>)) {
        self.symbols.iter().for_each(visitor);
    }

    #[inline]
    fn symbol_name<'exports>(
        &'exports self,
        symbol: &ElfSymbol<Arch::Layout>,
    ) -> Option<&'exports CStr> {
        self.names.get(symbol.st_name()).map(SymbolName::as_c_str)
    }

    #[inline]
    fn lookup<'exports>(
        &'exports self,
        lookup: &mut SymbolLookup<'_>,
    ) -> Option<&'exports ElfSymbol<Arch::Layout>> {
        let entry = self.index.get(lookup.name())?;
        let idx = match lookup.version_name() {
            Some(version) => entry
                .versions
                .iter()
                .find_map(|(entry, index)| (entry.name == version).then_some(*index))?,
            None => entry.default?,
        };
        Some(&self.symbols[idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{AddressSpaceId, Domain};
    use crate::tls::TlsResolver;
    use crate::{
        elf::ElfSymbolVisibility,
        image::ModuleScope,
        memory::{MappedRegion, VmOffset},
        segment::ElfSegments,
        tls::TlsTpOffset,
    };
    use core::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn synthetic_module_resolves_absolute_symbols_from_scope() {
        let module = SyntheticModule::<NativeArch>::new(
            &Domain::<NativeArch>::native(AddressSpaceId::PROCESS),
            "__bridge",
            [SyntheticSymbol::function(
                c"host_double",
                0x1234usize as *const (),
            )],
        );
        assert_eq!(
            <SyntheticModule<NativeArch> as Module<NativeArch>>::memory(&module).base(),
            VmAddr::null()
        );

        let mut scope = ModuleScope::<NativeArch>::new(
            Domain::<NativeArch>::native(AddressSpaceId::PROCESS).id(),
        );
        scope.extend([module]).unwrap();
        let mut lookup = SymbolLookup::new("host_double");

        let module = scope
            .iter()
            .find(|module| module.name() == b"__bridge")
            .expect("synthetic module should be retained in scope");
        assert_eq!(module.memory().base(), VmAddr::null());
        let symbol = module
            .exports()
            .lookup(&mut lookup)
            .expect("synthetic symbol should resolve");

        assert_eq!(symbol.st_value(), 0x1234);
        assert_eq!(symbol.st_size(), 0);
        assert_eq!(symbol.bind(), ElfSymbolBind::GLOBAL);
        assert_eq!(symbol.symbol_type(), ElfSymbolType::FUNC);
        assert!(symbol.st_shndx().is_abs());
        assert_eq!(module.exports().symbol_name(symbol), Some(c"host_double"));
    }

    #[test]
    fn resolve_hook_observes_address_and_propagates_errors() {
        let observed = Arc::new(AtomicUsize::new(0));
        let hook_observed = Arc::clone(&observed);
        let module = SyntheticModule::<NativeArch>::new(
            &Domain::<NativeArch>::native(AddressSpaceId::PROCESS),
            "__hook",
            [SyntheticSymbol::function(
                c"entry",
                0x1234usize as *const (),
            )],
        )
        .with_resolve_hook(move |symbol: &ElfSymbol<_>, address| {
            assert_eq!(symbol.st_value(), 0x1234);
            hook_observed.store(address.get(), Ordering::Relaxed);
            Ok(())
        });
        let mut lookup = SymbolLookup::new("entry");
        let symbol = module.lookup(&mut lookup).unwrap();

        let address = Module::<NativeArch>::resolve_symbol(&module, symbol).unwrap();
        assert_eq!(address, VmAddr::new(0x1234));
        assert_eq!(observed.load(Ordering::Relaxed), 0x1234);

        let failing = SyntheticModule::<NativeArch>::new(
            &Domain::<NativeArch>::native(AddressSpaceId::PROCESS),
            "__failing_hook",
            [SyntheticSymbol::function(
                c"entry",
                0x1234usize as *const (),
            )],
        )
        .with_resolve_hook(|_: &ElfSymbol<_>, _| Err(crate::custom_error("hook failed")));
        let symbol = failing.lookup(&mut lookup).unwrap();
        assert!(Module::<NativeArch>::resolve_symbol(&failing, symbol).is_err());
    }

    #[cfg(feature = "version")]
    #[test]
    fn synthetic_module_uses_one_default_symbol() {
        let mut module = SyntheticModule::<NativeArch>::new(
            &Domain::<NativeArch>::native(AddressSpaceId::PROCESS),
            "__versions",
            [SyntheticSymbol::function(
                c"entry",
                0x1000usize as *const (),
            )],
        );
        assert!(
            module
                .insert(SyntheticSymbol::from_fields(
                    c"entry",
                    0x2000,
                    0,
                    ElfSymbolBind::GLOBAL,
                    ElfSymbolType::FUNC,
                    0,
                    ElfSectionIndex::ABS,
                    Some(SymbolVersion::new("VER_1", false)),
                ))
                .is_none()
        );

        let mut lookup = SymbolLookup::new("entry");
        assert_eq!(module.lookup(&mut lookup).unwrap().st_value(), 0x1000);

        let mut lookup = SymbolLookup::with_version("entry", "VER_1");
        assert_eq!(module.lookup(&mut lookup).unwrap().st_value(), 0x2000);

        assert!(
            module
                .insert(
                    SyntheticSymbol::function(c"entry", 0x3000usize as *const ())
                        .with_version("VER_2", true),
                )
                .is_none()
        );

        let mut lookup = SymbolLookup::new("entry");
        assert_eq!(module.lookup(&mut lookup).unwrap().st_value(), 0x3000);

        let mut lookup = SymbolLookup::with_version("entry", "VER_2");
        assert_eq!(module.lookup(&mut lookup).unwrap().st_value(), 0x3000);

        let previous = module
            .insert(
                SyntheticSymbol::function(c"entry", 0x4000usize as *const ())
                    .with_version("VER_2", true),
            )
            .unwrap();
        assert_eq!(previous.value, 0x3000);
        assert_eq!(previous.version.unwrap().name(), b"VER_2");

        let previous = module
            .insert(SyntheticSymbol::function(
                c"entry",
                0x5000usize as *const (),
            ))
            .unwrap();
        assert_eq!(previous.value, 0x4000);
        assert_eq!(previous.version.unwrap().name(), b"VER_2");

        let mut lookup = SymbolLookup::new("entry");
        assert_eq!(module.lookup(&mut lookup).unwrap().st_value(), 0x5000);

        let mut lookup = SymbolLookup::with_version("entry", "VER_2");
        assert!(module.lookup(&mut lookup).is_none());
    }

    #[test]
    fn synthetic_symbol_can_use_non_absolute_section() {
        let module = SyntheticModule::<NativeArch>::new(
            &Domain::<NativeArch>::native(AddressSpaceId::PROCESS),
            "__tls",
            [SyntheticSymbol::from_fields(
                c"tls_slot",
                0x20,
                8,
                ElfSymbolBind::WEAK,
                ElfSymbolType::TLS,
                3,
                ElfSectionIndex::new(1),
                None,
            )],
        );
        let mut scope = ModuleScope::<NativeArch>::new(
            Domain::<NativeArch>::native(AddressSpaceId::PROCESS).id(),
        );
        scope.extend([module]).unwrap();
        let mut lookup = SymbolLookup::new("tls_slot");

        let module = scope
            .iter()
            .find(|module| module.name() == b"__tls")
            .expect("synthetic module should be retained in scope");
        let symbol = module
            .exports()
            .lookup(&mut lookup)
            .expect("synthetic TLS symbol should resolve");

        assert_eq!(symbol.st_value(), 0x20);
        assert_eq!(symbol.st_size(), 8);
        assert_eq!(symbol.bind(), ElfSymbolBind::WEAK);
        assert_eq!(symbol.symbol_type(), ElfSymbolType::TLS);
        assert_eq!(symbol.st_other(), 3);
        assert_eq!(symbol.visibility(), ElfSymbolVisibility::PROTECTED);
        assert_eq!(symbol.st_shndx(), ElfSectionIndex::new(1));
    }

    #[test]
    fn synthetic_module_carries_metadata() {
        #[derive(Clone, Debug, PartialEq, Eq)]
        struct SyntheticData {
            tag: usize,
        }

        let tls = crate::tls::DefaultTlsResolver::new()
            .register(
                crate::tls::TlsInfo::default(),
                crate::tls::TlsRequest::Static(Some(TlsTpOffset::new(-0x80))),
            )
            .unwrap();
        let binding = tls.module().binding();
        let mut module = SyntheticModule::<NativeArch>::empty(
            &Domain::native(AddressSpaceId::PROCESS)
                .with_tls(crate::tls::DefaultTlsResolver::new())
                .unwrap(),
            "__tls",
        )
        .with_tls(tls)
        .unwrap()
        .with_data(SyntheticData { tag: 7 });

        assert_eq!(
            <SyntheticModule<NativeArch, SyntheticData> as Module<NativeArch>>::tls(&module)
                .map(TlsModule::binding),
            Some(binding)
        );
        assert_eq!(module.user_data().tag, 7);
        module.user_data_mut().tag = 11;
        assert_eq!(module.user_data(), &SyntheticData { tag: 11 });
    }

    #[test]
    fn synthetic_module_can_delegate_image_memory() {
        let bytes = alloc::boxed::Box::leak(alloc::boxed::Box::new([1u8, 2, 3, 4]));
        let region = unsafe {
            MappedRegion::local_alias_no_unmap(bytes.as_ptr().cast_mut().cast(), bytes.len())
        };
        let base = VmAddr::from_ptr(bytes.as_ptr());
        let memory = ElfSegments::new(region, base, VmOffset::new(0));
        let module = SyntheticModule::<NativeArch>::empty(
            &Domain::<NativeArch>::native(AddressSpaceId::PROCESS),
            "__data",
        )
        .with_memory(memory);
        let memory = <SyntheticModule<NativeArch> as Module<NativeArch>>::memory(&module);
        let mut out = [0u8; 2];

        memory
            .read_bytes(base + VmOffset::new(1), &mut out)
            .expect("synthetic module should delegate readable image memory");

        assert_eq!(memory.base(), base);
        assert_eq!(out, [2, 3]);
    }
}
