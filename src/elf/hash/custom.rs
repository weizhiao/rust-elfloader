//! Custom ELF hash table implementation
//!
//! This module provides a custom hash table implementation that can be used
//! when standard ELF hash sections (.hash or .gnu.hash) are not available.
//! It uses the hashbrown crate for efficient hash table operations.

use super::ElfHashTable;
use crate::{
    elf::{ElfShdr, ElfSymbol},
    elf::{ElfStringTable, PreCompute, SymbolTable, symbol::SymbolInfo},
};
use core::hash::{Hash, Hasher};
use elf::abi::STT_FILE;
use foldhash::{SharedSeed, fast::FoldHasher};
use hashbrown::HashTable;

struct TableEntry {
    name: &'static str,
    idx: usize,
}

const HASHER: FoldHasher<'static> = FoldHasher::with_seed(0, SharedSeed::global_fixed());

/// Custom ELF hash table implementation
///
/// This structure implements a hash table using the hashbrown HashMap,
/// which provides excellent performance and is used as a fallback when
/// standard ELF hash sections are not available.
pub(crate) struct CustomHash {
    /// Hash map from symbol names to symbol indices
    map: HashTable<TableEntry>,
}

impl CustomHash {
    /// Create a custom hash table from section headers
    ///
    /// This method creates a custom hash table by iterating through the
    /// symbols in the symbol table and building a hash map from symbol
    /// names to their indices.
    ///
    /// # Arguments
    /// * `symtab` - The symbol table section header
    /// * `strtab` - The string table
    ///
    /// # Returns
    /// A CustomHash instance containing the hash map
    pub(crate) fn from_shdr(symtab: &ElfShdr, strtab: &ElfStringTable) -> Self {
        // Get mutable access to the symbols
        let symbols: &mut [ElfSymbol] = symtab.content_mut();

        // Create a hash map with capacity for all symbols
        let mut map = HashTable::with_capacity(symbols.len());

        // Populate the hash map with symbol names and indices
        for (idx, symbol) in symbols.iter_mut().enumerate() {
            // Skip file symbols as they're not typically looked up
            if symbol.st_type() == STT_FILE {
                continue;
            }

            // Get the symbol name and add it to the hash map
            let name = strtab.get_str(symbol.st_name() as usize);
            let hash = Self::hash(name.as_bytes());
            map.insert_unique(hash, TableEntry { name, idx }, |val| {
                Self::hash(val.name.as_bytes())
            });
        }

        Self { map }
    }
}

impl ElfHashTable for CustomHash {
    /// Compute a hash value for a symbol name using the default hasher
    ///
    /// This method uses the default hashbrown hasher to compute a hash
    /// value for the given symbol name.
    ///
    /// # Arguments
    /// * `name` - The symbol name as a byte slice
    ///
    /// # Returns
    /// The computed hash value
    fn hash(name: &[u8]) -> u64 {
        let mut hasher = HASHER.clone();
        name.hash(&mut hasher);
        hasher.finish()
    }

    /// Get the number of symbols in the hash table
    ///
    /// # Returns
    /// The number of symbols in the hash table
    fn count_syms(&self) -> usize {
        self.map.len()
    }

    /// Look up a symbol in the custom hash table
    ///
    /// This method performs a symbol lookup using the hashbrown HashMap,
    /// utilizing precomputed hash values from the PreCompute structure
    /// for improved performance.
    ///
    /// # Arguments
    /// * `table` - The symbol table to search in
    /// * `symbol` - Information about the symbol to look up
    /// * `precompute` - Precomputed hash values for faster lookup
    ///
    /// # Returns
    /// * `Some(symbol)` - A reference to the found symbol
    /// * `None` - If the symbol was not found
    fn lookup<'sym>(
        table: &'sym SymbolTable,
        symbol: &SymbolInfo,
        precompute: &mut PreCompute,
    ) -> Option<&'sym ElfSymbol> {
        // Get reference to the custom hash table
        let custom_hash = table.hashtab.into_customhash().unwrap();
        let name = symbol.name();
        // Get or compute the hash value for the symbol
        let hash = if let Some(hash) = precompute.custom {
            hash
        } else {
            let hash = Self::hash(name.as_bytes());
            precompute.custom = Some(hash);
            hash
        };

        // Try to find the symbol using the precomputed hash
        custom_hash
            .map
            .find(hash, |entry| entry.name == name)
            .map(|entry| table.symbol_idx(entry.idx).0)
    }
}
