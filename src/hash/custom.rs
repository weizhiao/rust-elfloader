//! Custom ELF hash table implementation
//!
//! This module provides a custom hash table implementation that can be used
//! when standard ELF hash sections (.hash or .gnu.hash) are not available.
//! It uses the hashbrown crate for efficient hash table operations.

use core::hash::BuildHasher;

use alloc::vec::Vec;
use elf::abi::STT_FILE;
use hashbrown::{DefaultHashBuilder, HashMap};

use crate::{
    arch::{ElfShdr, ElfSymbol},
    hash::{ElfHashTable, PreCompute},
    symbol::{ElfStringTable, SymbolInfo, SymbolTable},
};

/// Custom ELF hash table implementation
///
/// This structure implements a hash table using the hashbrown HashMap,
/// which provides excellent performance and is used as a fallback when
/// standard ELF hash sections are not available.
pub(crate) struct CustomHash {
    /// Hash map from symbol names to symbol indices
    map: HashMap<Vec<u8>, usize>,
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
        let mut map =
            HashMap::with_capacity_and_hasher(symbols.len(), DefaultHashBuilder::default());

        // Populate the hash map with symbol names and indices
        for (idx, symbol) in symbols.iter_mut().enumerate() {
            // Skip file symbols as they're not typically looked up
            if symbol.st_type() == STT_FILE {
                continue;
            }

            // Get the symbol name and add it to the hash map
            let name = strtab.get_str(symbol.st_name() as usize);
            map.insert(name.as_bytes().to_vec(), idx);
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
        DefaultHashBuilder::default().hash_one(name)
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
    /// This method performs a symbol lookup using the hashbrown HashMap.
    ///
    /// # Arguments
    /// * `table` - The symbol table to search in
    /// * `symbol` - Information about the symbol to look up
    /// * `_precompute` - Precomputed hash values (unused in this implementation)
    ///
    /// # Returns
    /// * `Some(symbol)` - A reference to the found symbol
    /// * `None` - If the symbol was not found
    fn lookup<'sym>(
        table: &'sym SymbolTable,
        symbol: &SymbolInfo,
        _precompute: &mut PreCompute,
    ) -> Option<&'sym ElfSymbol> {
        // TODO: optimize
        table
            .hashtab
            .into_customhash()
            .unwrap()
            .map
            .get(symbol.name().as_bytes())
            .map(|idx| table.symbol_idx(*idx).0)
    }
}
