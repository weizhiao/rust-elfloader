//! Address ownership lookup without retaining mapped images.

use alloc::vec::Vec;
use core::ops::Range;

use super::ModuleId;
use crate::memory::{ImageMemory, VmAddr};

/// A committed module and the mapped interval containing the queried address.
/// This value does not retain the module or its memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddressMatch {
    /// Identity of the committed module owning this interval.
    pub owner: ModuleId,
    /// Actual mapped interval containing the queried address.
    pub range: Range<VmAddr>,
}

#[derive(Clone)]
struct Entry {
    owner: ModuleId,
    range: Range<VmAddr>,
}

/// Index over non-overlapping mappings of committed modules.
///
/// The caller supplies module identities and serializes mutations with lookups.
/// Images must remain alive while registered, and must be removed before their
/// mappings become invalid. Module IDs identify individual incarnations of an image.
/// The index stores no image references and imposes no locking policy.
///
/// Mapped intervals must not overlap within an index. Lookup costs O(log R).
#[derive(Clone, Default)]
pub struct AddressIndex {
    entries: Vec<Entry>,
}

impl AddressIndex {
    /// Adds an image's non-empty ranges. Overlapping intervals panic.
    pub fn insert(&mut self, owner: ModuleId, memory: &dyn ImageMemory) {
        self.extend(core::iter::once((owner, memory)));
    }

    /// Adds a batch of image ranges, sorting the index only once.
    /// Overlapping intervals panic, including ranges belonging to the same module.
    /// Images without non-empty ranges do not change the index.
    pub fn extend<'a>(
        &mut self,
        images: impl IntoIterator<Item = (ModuleId, &'a dyn ImageMemory)>,
    ) {
        for (owner, memory) in images {
            memory.visit_ranges(&mut |range| {
                if !range.is_empty() {
                    self.entries.push(Entry { owner, range });
                }
            });
        }
        self.entries.sort_unstable_by_key(|entry| entry.range.start);
        assert!(
            self.entries
                .windows(2)
                .all(|pair| pair[0].range.end <= pair[1].range.start),
            "module address ranges must not overlap"
        );
    }

    /// Removes an owner and all its ranges. An absent key is a no-op.
    pub fn remove(&mut self, owner: ModuleId) {
        self.retain(|key| key != owner);
    }

    /// Retains ranges whose owners satisfy `keep`, without re-reading image mappings.
    /// The predicate is called once per range and should return the same decision
    /// for every range belonging to the same module.
    pub fn retain(&mut self, mut keep: impl FnMut(ModuleId) -> bool) {
        self.entries.retain(|entry| keep(entry.owner));
    }

    /// Finds the mapped interval containing the address.
    pub fn find(&self, address: VmAddr) -> Option<(ModuleId, Range<VmAddr>)> {
        let cursor = self
            .entries
            .partition_point(|entry| entry.range.start <= address);
        let entry = self.entries.get(cursor.checked_sub(1)?)?;
        entry
            .range
            .contains(&address)
            .then(|| (entry.owner, entry.range.clone()))
    }
}
