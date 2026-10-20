//! Custom ELF hash table implementation for relocatable objects.

use crate::elf::{
    ElfLayout, ElfStringTable, ElfSymbol, ElfSymbolType, SymbolHash, SymbolLookup, SymbolTableView,
};
use hashbrown::HashTable as RawHashTable;

struct TableEntry<N> {
    hash: u64,
    name: N,
    idx: usize,
}

/// Hash table used for symbol lookup in Relink-generated object exports.
pub struct CustomHash<N = &'static [u8]> {
    map: RawHashTable<TableEntry<N>>,
}

impl<N> CustomHash<N> {
    #[inline]
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            map: RawHashTable::with_capacity(capacity),
        }
    }

    #[inline]
    pub(crate) fn count_syms(&self) -> usize {
        self.map.len()
    }
}

impl<N: AsRef<[u8]>> CustomHash<N> {
    #[inline]
    pub(crate) fn find_idx(&self, name: &[u8]) -> Option<usize> {
        let hash = SymbolLookup::new(name).index_hash();
        self.find_idx_with_hash(name, hash)
    }

    #[inline]
    pub(crate) fn lookup_idx(&self, lookup: &mut SymbolLookup<'_>) -> Option<usize> {
        let hash = lookup.index_hash();
        self.find_idx_with_hash(lookup.name(), hash)
    }

    #[inline]
    pub(crate) fn insert_unique(&mut self, name: N, idx: usize) {
        let hash = SymbolLookup::new(name.as_ref()).index_hash();
        self.map
            .insert_unique(hash, TableEntry { hash, name, idx }, |entry| entry.hash);
    }

    #[inline]
    fn find_idx_with_hash(&self, name: &[u8], hash: u64) -> Option<usize> {
        self.map
            .find(hash, |entry| entry.name.as_ref() == name)
            .map(|entry| entry.idx)
    }
}

impl CustomHash<&'static [u8]> {
    pub(crate) fn from_symbols<L: ElfLayout>(
        symbols: &[ElfSymbol<L>],
        strtab: &ElfStringTable,
    ) -> Self {
        let mut hashtab = Self::with_capacity(symbols.len());

        for (idx, symbol) in symbols.iter().enumerate() {
            if symbol.symbol_type() == ElfSymbolType::FILE || symbol.is_undef() {
                continue;
            }

            let name = strtab.get_cstr(symbol.st_name()).to_bytes();
            hashtab.insert_unique(name, idx);
        }

        hashtab
    }
}

impl<L: ElfLayout> SymbolHash<L> for CustomHash {
    fn lookup<'sym, H>(
        &self,
        table: SymbolTableView<'sym, L, H>,
        lookup: &mut SymbolLookup<'_>,
    ) -> Option<&'sym ElfSymbol<L>> {
        self.lookup_idx(lookup)
            .and_then(|index| table.get_raw(index))
    }
}
