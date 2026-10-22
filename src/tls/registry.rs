use super::{TlsBinding, TlsInfo, TlsModuleId, TlsProvider, TlsTemplate, TlsTpOffset};
use crate::{
    Result, TlsError,
    sync::{Arc, arc_unsize},
};
use alloc::{
    collections::{BTreeMap, btree_map::Entry},
    vec::Vec,
};
use core::{fmt, ptr::NonNull};

/// Immutable metadata assigned to a provider by its registry.
#[derive(Clone, Copy, Debug)]
pub struct TlsContext {
    pub(crate) info: TlsInfo,
    pub(crate) binding: TlsBinding,
}

impl TlsContext {
    /// Returns the registered TLS segment metadata.
    pub const fn info(&self) -> TlsInfo {
        self.info
    }

    /// Returns this registration's relocation metadata.
    pub const fn binding(&self) -> TlsBinding {
        self.binding
    }
}

/// Keeps a TLS provider alive independently of its registry entry.
///
/// Create this before taking the registry lock, and drop the last owner outside
/// that lock: provider destruction may call back into the runtime. This handle
/// does not keep the module ID registered.
#[derive(Clone)]
pub struct TlsOwner(pub(crate) Arc<dyn TlsProvider>);

impl TlsOwner {
    /// Retains a provider independently of its lookup entry.
    pub fn new<P: TlsProvider>(provider: P) -> Self {
        Self(arc_unsize!(Arc::new(provider) => dyn TlsProvider))
    }
}

impl fmt::Debug for TlsOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsOwner").finish_non_exhaustive()
    }
}

/// A provider bound to the immutable metadata of one registration.
///
/// Clone an entry under the registry lock, then release the lock before calling
/// its methods or dropping the last copy. An entry retains provider resources,
/// but does not keep its module ID registered.
#[derive(Clone, Debug)]
pub struct TlsRegistryEntry {
    context: TlsContext,
    owner: TlsOwner,
}

impl TlsRegistryEntry {
    /// Returns the TLS segment metadata.
    pub const fn info(&self) -> TlsInfo {
        self.context.info()
    }

    /// Returns the registration's relocation metadata.
    pub const fn binding(&self) -> TlsBinding {
        self.context.binding()
    }

    /// Publishes a relocated template according to the provider's policy.
    /// Call at most once per registration, including across clones of this entry.
    /// Repeated publication may panic.
    pub fn publish(&self, source: TlsTemplate) -> Result<()> {
        if source.len() != self.context.info().filesz {
            return Err(TlsError::InvalidInfo.into());
        }
        // SAFETY: Registry construction binds this context to this provider.
        unsafe { self.owner.0.publish(&self.context, source) }
    }

    /// Resolves a logical offset in the current thread's block.
    pub fn resolve(&self, offset: usize) -> Result<NonNull<u8>> {
        // SAFETY: Registry construction binds this context to this provider.
        unsafe { self.owner.0.resolve(&self.context, offset) }
    }

    /// Queries the current thread's block without allocating TLS.
    pub fn get(&self) -> Option<NonNull<u8>> {
        // SAFETY: Registry construction binds this context to this provider.
        unsafe { self.owner.0.get(&self.context) }
    }
}

/// Live module IDs used to discard stale per-thread blocks.
#[derive(Clone, Debug)]
pub struct TlsRegistrySnapshot {
    /// Registry membership generation.
    pub generation: usize,
    /// All live module IDs in ascending order, without retaining their owners.
    pub modules: Vec<TlsModuleId>,
}

/// Stores TLS providers under IDs that are never reused.
/// Clone an entry and release the registry lock before accessing its storage.
#[derive(Debug)]
pub struct TlsRegistry {
    modules: BTreeMap<TlsModuleId, TlsRegistryEntry>,
    next_id: usize,
    generation: usize,
}

impl TlsRegistry {
    /// Creates a registry; ID zero is reserved by the TLS ABI.
    pub const fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
            next_id: 1,
            generation: 0,
        }
    }

    /// Returns the registry membership generation.
    pub const fn generation(&self) -> usize {
        self.generation
    }

    /// Registers a provider in this runtime's module-ID namespace.
    ///
    /// The borrowed owner ensures that a failed registration cannot destroy the
    /// provider while the caller holds a registry lock. No provider callback is
    /// invoked here. Publish managed templates through the returned ID's entry.
    ///
    /// # Safety
    /// `info` must describe this provider's TLS block. Any `tp_offset` must locate
    /// that block at the same offset in every current and future calling thread.
    /// The provider must support this registry's namespace and thread storage.
    pub unsafe fn register(
        &mut self,
        info: TlsInfo,
        tp_offset: Option<TlsTpOffset>,
        owner: &TlsOwner,
    ) -> Result<TlsBinding> {
        info.validate()?;
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or(TlsError::ResolverUnsupported)?;
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(TlsError::ResolverUnsupported)?;
        let binding = TlsBinding::new(TlsModuleId::new(self.next_id), tp_offset);
        self.modules.insert(
            binding.mod_id(),
            TlsRegistryEntry {
                context: TlsContext { info, binding },
                owner: owner.clone(),
            },
        );
        self.next_id = next_id;
        self.generation = generation;
        Ok(binding)
    }

    /// Detaches a registration. Drop its returned owner after releasing locks.
    pub fn unregister(&mut self, mod_id: TlsModuleId) -> Option<TlsOwner> {
        let Entry::Occupied(entry) = self.modules.entry(mod_id) else {
            return None;
        };
        // Finish fallible work before detaching an owner: unwinding must not
        // destroy a provider while the caller still holds the registry lock.
        let generation = self
            .generation
            .checked_add(1)
            .expect("TLS generation exhausted");
        let entry = entry.remove();
        self.generation = generation;
        Some(entry.owner)
    }

    /// Returns a bound entry for use after releasing the registry lock.
    pub fn lookup(&self, mod_id: TlsModuleId) -> Result<TlsRegistryEntry> {
        self.modules
            .get(&mod_id)
            .cloned()
            .ok_or_else(|| TlsError::InvalidModuleId { mod_id }.into())
    }

    /// Returns whether the module is still registered.
    pub fn contains(&self, mod_id: TlsModuleId) -> bool {
        self.modules.contains_key(&mod_id)
    }

    /// Returns segment metadata for a live module.
    pub fn info(&self, mod_id: TlsModuleId) -> Option<TlsInfo> {
        self.modules.get(&mod_id).map(TlsRegistryEntry::info)
    }

    /// Returns live IDs only when registry membership changed.
    pub fn snapshot_since(&self, generation: usize) -> Option<TlsRegistrySnapshot> {
        (self.generation != generation).then(|| self.snapshot())
    }

    /// Returns all live IDs without retaining storage owners or templates.
    pub fn snapshot(&self) -> TlsRegistrySnapshot {
        TlsRegistrySnapshot {
            generation: self.generation,
            modules: self.modules.keys().copied().collect(),
        }
    }
}

impl Default for TlsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tls::{TlsTemplate, manager::ManagedTls};

    fn info() -> TlsInfo {
        TlsInfo {
            vaddr: 0,
            filesz: 4,
            memsz: 8,
            align: 4,
        }
    }

    #[test]
    fn unregister_changes_generation_without_reusing_ids() {
        let mut registry = TlsRegistry::new();
        let first =
            unsafe { registry.register(info(), None, &TlsOwner::new(ManagedTls::new())) }.unwrap();
        let first_id = first.mod_id();
        assert_eq!(first_id.get(), 1);

        let generation = registry.generation();
        assert!(registry.snapshot_since(generation).is_none());
        assert!(registry.unregister(first_id).is_some());
        assert!(registry.generation() > generation);
        assert!(!registry.contains(first_id));

        let offset = TlsTpOffset::new(-8);
        let second =
            unsafe { registry.register(info(), Some(offset), &TlsOwner::new(ManagedTls::new())) }
                .unwrap();
        assert_ne!(second.mod_id(), first_id);
        assert_eq!(second.tp_offset(), Some(offset));
    }

    #[test]
    fn rejects_invalid_tls_info() {
        let mut registry = TlsRegistry::new();
        let invalid = TlsInfo { align: 3, ..info() };
        assert!(
            unsafe { registry.register(invalid, None, &TlsOwner::new(ManagedTls::new())) }.is_err()
        );
        assert_eq!(registry.generation(), 0);
    }

    #[test]
    #[should_panic(expected = "TLS template must be published only once")]
    fn rejects_repeated_publication() {
        let mut registry = TlsRegistry::new();
        let module =
            unsafe { registry.register(info(), None, &TlsOwner::new(ManagedTls::new())) }.unwrap();
        let source = TlsTemplate::new(&[0; 4]);

        let owner = registry.lookup(module.mod_id()).unwrap();
        owner.publish(source.clone()).unwrap();
        let _ = owner.publish(source);
    }
}
