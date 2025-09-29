use core::hash::BuildHasher;

use alloc::vec::Vec;
use elf::abi::STT_FILE;
use hashbrown::{DefaultHashBuilder, HashMap};

use crate::{
    arch::{ElfShdr, ElfSymbol},
    hash::{ElfHashTable, PreCompute},
    symbol::{ElfStringTable, SymbolInfo, SymbolTable},
};

pub(crate) struct CustomHash {
    map: HashMap<Vec<u8>, usize>,
}

impl CustomHash {
    pub(crate) fn from_shdr(symtab: &ElfShdr, strtab: &ElfStringTable) -> Self {
        let symbols: &[ElfSymbol] = symtab.content();
        let mut map =
            HashMap::with_capacity_and_hasher(symbols.len(), DefaultHashBuilder::default());
        for (idx, symbol) in symbols.iter().enumerate() {
            if symbol.st_type() == STT_FILE {
                continue;
            }
            let name = strtab.get_str(symbol.st_name() as usize);
            map.insert(name.as_bytes().to_vec(), idx);
        }
        Self { map }
    }
}

impl ElfHashTable for CustomHash {
    fn hash(name: &[u8]) -> u64 {
        DefaultHashBuilder::default().hash_one(name)
    }

    fn count_syms(&self) -> usize {
        self.map.len()
    }

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
