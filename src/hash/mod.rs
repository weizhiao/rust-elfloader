use crate::{
    arch::{ElfShdr, ElfSymbol},
    dynamic::{ElfDynamic, ElfDynamicHashTab},
    hash::{custom::CustomHash, gnu::ElfGnuHash, sysv::ElfHash},
    symbol::{ElfStringTable, SymbolInfo, SymbolTable},
};

mod custom;
mod gnu;
mod sysv;

pub(crate) trait ElfHashTable {
    fn hash(name: &[u8]) -> u64;
    fn count_syms(&self) -> usize;
    fn lookup<'sym>(
        table: &'sym SymbolTable,
        symbol: &SymbolInfo,
        precompute: &mut PreCompute,
    ) -> Option<&'sym ElfSymbol>;
}

pub(crate) enum HashTable {
    /// .gnu.hash
    Gnu(ElfGnuHash),
    /// .hash
    Elf(ElfHash),
    /// custom
    Custom(CustomHash),
}

pub struct PreCompute {
    gnuhash: u32,
    fofs: usize,
    fmask: usize,
    hash: Option<u32>,
    _custom: Option<u64>,
}

impl HashTable {
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn hash(&self, name: &[u8]) -> u64 {
        match &self {
            HashTable::Gnu(_) => ElfGnuHash::hash(name),
            HashTable::Elf(_) => ElfHash::hash(name),
            HashTable::Custom(_) => CustomHash::hash(name),
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub(crate) fn count_syms(&self) -> usize {
        match &self {
            HashTable::Gnu(hashtab) => hashtab.count_syms(),
            HashTable::Elf(hashtab) => hashtab.count_syms(),
            HashTable::Custom(hashtab) => hashtab.count_syms(),
        }
    }

    pub(crate) fn lookup<'sym>(
        &self,
        table: &'sym SymbolTable,
        symbol: &SymbolInfo,
        precompute: &mut PreCompute,
    ) -> Option<&'sym ElfSymbol> {
        match self {
            HashTable::Gnu(_) => ElfGnuHash::lookup(table, symbol, precompute),
            HashTable::Elf(_) => ElfHash::lookup(table, symbol, precompute),
            HashTable::Custom(_) => CustomHash::lookup(table, symbol, precompute),
        }
    }

    pub(crate) fn from_shdr(symtab: &ElfShdr, strtab: &ElfStringTable) -> Self {
        HashTable::Custom(CustomHash::from_shdr(symtab, strtab))
    }

    pub(crate) fn from_dynamic(dynamic: &ElfDynamic) -> Self {
        match dynamic.hashtab {
            ElfDynamicHashTab::Gnu(off) => HashTable::Gnu(ElfGnuHash::parse(off as *const u8)),
            ElfDynamicHashTab::Elf(off) => HashTable::Elf(ElfHash::parse(off as *const u8)),
        }
    }

    fn into_gnuhash(&self) -> Option<&ElfGnuHash> {
        match self {
            HashTable::Gnu(hashtab) => Some(hashtab),
            _ => None,
        }
    }

    fn into_elfhash(&self) -> Option<&ElfHash> {
        match self {
            HashTable::Elf(hashtab) => Some(hashtab),
            _ => None,
        }
    }

    fn into_customhash(&self) -> Option<&CustomHash> {
        match self {
            HashTable::Custom(hashtab) => Some(hashtab),
            _ => None,
        }
    }
}

impl SymbolInfo<'_> {
    #[inline]
    pub fn precompute(&self) -> PreCompute {
        let gnuhash = ElfGnuHash::hash(self.name().as_bytes()) as u32;
        PreCompute {
            gnuhash,
            fofs: gnuhash as usize / usize::BITS as usize,
            fmask: 1 << (gnuhash % (8 * size_of::<usize>() as u32)),
            hash: None,
            _custom: None,
        }
    }
}
