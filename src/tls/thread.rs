use super::{TlsInfo, TlsModuleId, TlsRegistrySnapshot};
use crate::{Result, TlsError};
use alloc::{
    alloc::{alloc_zeroed, dealloc, handle_alloc_error},
    collections::BTreeMap,
};
use core::{alloc::Layout, ptr::NonNull};

// Owns only the allocation; template and module lifetimes stay with the caller.
struct TlsBlock {
    ptr: NonNull<u8>,
    layout: Layout,
}

unsafe impl Send for TlsBlock {}

impl TlsBlock {
    fn new(info: TlsInfo, image: &[u8]) -> Result<Self> {
        info.validate()?;
        if image.len() != info.filesz {
            return Err(TlsError::ModuleMismatch.into());
        }
        let layout = Layout::from_size_align(info.memsz.max(1), info.align.max(1))
            .map_err(|_| TlsError::InvalidInfo)?;
        let ptr = NonNull::new(unsafe { alloc_zeroed(layout) })
            .unwrap_or_else(|| handle_alloc_error(layout));
        unsafe { core::ptr::copy_nonoverlapping(image.as_ptr(), ptr.as_ptr(), image.len()) };
        Ok(Self { ptr, layout })
    }
}

impl Drop for TlsBlock {
    fn drop(&mut self) {
        unsafe { dealloc(self.ptr.as_ptr(), self.layout) };
    }
}

/// Per-thread cache of dynamically allocated TLS blocks.
///
/// Static and external storage remain with their providers. This type does not
/// define an architecture's ABI-visible DTV layout and retains no module owners.
pub struct ThreadDtv {
    generation: usize,
    blocks: BTreeMap<TlsModuleId, TlsBlock>,
}

impl ThreadDtv {
    /// Creates an empty per-thread DTV.
    pub const fn new() -> Self {
        Self {
            generation: 0,
            blocks: BTreeMap::new(),
        }
    }

    /// Returns the registry generation represented by this DTV.
    #[inline]
    pub const fn generation(&self) -> usize {
        self.generation
    }

    /// Discards blocks whose IDs are absent from a sorted registry snapshot.
    /// This never allocates a block or retains a storage owner.
    pub fn sync(&mut self, snapshot: &TlsRegistrySnapshot) {
        self.blocks
            .retain(|id, _| snapshot.modules.binary_search(id).is_ok());
        self.generation = snapshot.generation;
    }

    /// Returns a cached block, initializing it from template bytes on first use.
    ///
    /// Module IDs must not be reused for different registrations. The caller
    /// keeps the template alive until this borrow ends; this operation invokes
    /// no image-provider callbacks and releases no module owners.
    pub fn get_or_init(
        &mut self,
        mod_id: TlsModuleId,
        info: TlsInfo,
        image: &[u8],
    ) -> Result<NonNull<u8>> {
        use alloc::collections::btree_map::Entry;
        let block = match self.blocks.entry(mod_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(TlsBlock::new(info, image)?),
        };
        Ok(block.ptr)
    }

    /// Returns an already allocated dynamic block.
    pub fn get(&self, mod_id: TlsModuleId) -> Option<NonNull<u8>> {
        self.blocks.get(&mod_id).map(|block| block.ptr)
    }
}

impl Default for ThreadDtv {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn info() -> TlsInfo {
        TlsInfo {
            vaddr: 0,
            filesz: 4,
            memsz: 16,
            align: 8,
        }
    }

    #[test]
    fn sync_discards_unregistered_blocks() {
        let mod_id = TlsModuleId::new(1);
        let mut dtv = ThreadDtv::new();
        let address = dtv.get_or_init(mod_id, info(), &[1, 2, 3, 4]).unwrap();
        dtv.sync(&TlsRegistrySnapshot {
            generation: 1,
            modules: alloc::vec![mod_id],
        });
        assert_eq!(dtv.get(mod_id), Some(address));

        dtv.sync(&TlsRegistrySnapshot {
            generation: 2,
            modules: alloc::vec![],
        });
        assert_eq!(dtv.get(mod_id), None);
    }

    #[test]
    fn dynamic_blocks_are_allocated_lazily() {
        let mod_id = TlsModuleId::new(1);
        let mut dtv = ThreadDtv::new();
        dtv.sync(&TlsRegistrySnapshot {
            generation: 1,
            modules: alloc::vec![mod_id],
        });
        assert_eq!(dtv.get(mod_id), None);
        let address = dtv.get_or_init(mod_id, info(), &[1, 2, 3, 4]).unwrap();
        assert_eq!(dtv.get(mod_id), Some(address));
        // A cache hit must not initialize or validate a replacement template.
        assert_eq!(dtv.get_or_init(mod_id, info(), &[]).unwrap(), address);
    }

    #[test]
    fn blocks_copy_the_template_and_zero_the_remainder() {
        let block = TlsBlock::new(info(), &[1, 2, 3, 4]).unwrap();
        let bytes = unsafe { core::slice::from_raw_parts(block.ptr.as_ptr(), info().memsz) };
        assert_eq!(&bytes[..4], &[1, 2, 3, 4]);
        assert_eq!(&bytes[4..], &[0; 12]);
    }
}
