use crate::{
    arch::ElfSymbol,
    dynamic::{ElfDynamic, ElfDynamicHashTab},
};
use core::ffi::CStr;

#[repr(C)]
struct ElfGnuHeader {
    nbucket: u32,
    symbias: u32,
    nbloom: u32,
    nshift: u32,
}

pub(crate) struct ElfGnuHash {
    header: ElfGnuHeader,
    blooms: *const usize,
    buckets: *const u32,
    chains: *const u32,
}

trait ElfHashTable {
    fn hash(name: &[u8]) -> u32;
    fn count_syms(&self) -> usize;
}

impl ElfGnuHash {
    #[inline]
    pub(crate) fn parse(ptr: *const u8) -> ElfGnuHash {
        const HEADER_SIZE: usize = size_of::<ElfGnuHeader>();
        let mut bytes = [0u8; HEADER_SIZE];
        bytes.copy_from_slice(unsafe { core::slice::from_raw_parts(ptr, HEADER_SIZE) });
        let header: ElfGnuHeader = unsafe { core::mem::transmute(bytes) };
        let bloom_size = header.nbloom as usize * size_of::<usize>();
        let bucket_size = header.nbucket as usize * size_of::<u32>();

        let blooms = unsafe { ptr.add(HEADER_SIZE) };
        let buckets = unsafe { blooms.add(bloom_size) };
        let chains = unsafe { buckets.add(bucket_size) };
        ElfGnuHash {
            header,
            blooms: blooms.cast(),
            buckets: buckets.cast(),
            chains: chains.cast(),
        }
    }
}

impl ElfHashTable for ElfGnuHash {
    #[inline]
    fn hash(name: &[u8]) -> u32 {
        let mut hash = 5381u32;
        for byte in name {
            hash = hash.wrapping_mul(33).wrapping_add(u32::from(*byte));
        }
        hash
    }

    fn count_syms(&self) -> usize {
        let mut nsym = 0;
        for i in 0..self.header.nbucket as usize {
            nsym = nsym.max(unsafe { self.buckets.add(i).read() as usize });
        }
        if nsym > 0 {
            unsafe {
                let mut hashval = self.chains.add(nsym - self.header.symbias as usize);
                while hashval.read() & 1 == 0 {
                    nsym += 1;
                    hashval = hashval.add(1);
                }
            }
        }
        nsym + 1
    }
}

#[repr(C)]
struct ElfHashHeader {
    nbucket: u32,
    nchain: u32,
}

pub(crate) struct ElfHash {
    header: ElfHashHeader,
    buckets: *const u32,
    chains: *const u32,
}

impl ElfHash {
    #[inline]
    pub(crate) fn parse(ptr: *const u8) -> ElfHash {
        const HEADER_SIZE: usize = size_of::<ElfHashHeader>();
        let mut bytes = [0u8; HEADER_SIZE];
        bytes.copy_from_slice(unsafe { core::slice::from_raw_parts(ptr, HEADER_SIZE) });
        let header: ElfHashHeader = unsafe { core::mem::transmute(bytes) };
        let bucket_size = header.nbucket as usize * size_of::<u32>();

        let buckets = unsafe { ptr.add(HEADER_SIZE) };
        let chains = unsafe { buckets.add(bucket_size) };
        ElfHash {
            header,
            buckets: buckets.cast(),
            chains: chains.cast(),
        }
    }
}

impl ElfHashTable for ElfHash {
    #[inline]
    fn hash(name: &[u8]) -> u32 {
        let mut hash = 0u32;
        #[allow(unused_assignments)]
        let mut g = 0u32;
        for byte in name {
            hash = (hash << 4) + u32::from(*byte);
            g = hash & 0xf0000000;
            if g != 0 {
                hash ^= g >> 24;
            }
            hash &= !g;
        }
        hash
    }

    #[inline]
    fn count_syms(&self) -> usize {
        self.header.nchain as usize
    }
}

pub(crate) struct ElfStringTable {
    data: *const u8,
}

impl ElfStringTable {
    const fn new(data: *const u8) -> Self {
        ElfStringTable { data }
    }

    #[inline]
    pub(crate) fn get_cstr(&self, offset: usize) -> &'static CStr {
        unsafe {
            let start = self.data.add(offset).cast();
            CStr::from_ptr(start)
        }
    }

    #[inline]
    fn convert_cstr(s: &CStr) -> &str {
        unsafe { core::str::from_utf8_unchecked(s.to_bytes()) }
    }

    #[inline]
    pub(crate) fn get_str(&self, offset: usize) -> &'static str {
        Self::convert_cstr(self.get_cstr(offset))
    }
}

pub(crate) enum SymbolTableHashTable {
    /// .gnu.hash
    Gnu(ElfGnuHash),
    /// .hash
    Elf(ElfHash),
}

impl SymbolTableHashTable {
    #[inline]
    #[allow(dead_code)]
    fn hash(&self, name: &[u8]) -> u32 {
        match &self {
            SymbolTableHashTable::Gnu(_) => ElfGnuHash::hash(name),
            SymbolTableHashTable::Elf(_) => ElfHash::hash(name),
        }
    }

    #[inline]
    #[allow(dead_code)]
    fn count_syms(&self) -> usize {
        match &self {
            SymbolTableHashTable::Gnu(hashtab) => hashtab.count_syms(),
            SymbolTableHashTable::Elf(hashtab) => hashtab.count_syms(),
        }
    }
}

/// Symbol table of elf file.
pub struct SymbolTable {
    /// .gnu.hash / .hash
    pub(crate) hashtab: SymbolTableHashTable,
    /// .dynsym
    symtab: *const ElfSymbol,
    /// .dynstr
    strtab: ElfStringTable,
    #[cfg(feature = "version")]
    /// .gnu.version
    pub(crate) version: Option<super::version::ELFVersion>,
}

/// Symbol specific information, including symbol name and version name.
pub struct SymbolInfo<'symtab> {
    name: &'symtab str,
    cname: Option<&'symtab CStr>,
    #[cfg(feature = "version")]
    version: Option<super::version::SymbolVersion<'symtab>>,
}

pub struct PreCompute {
    gnuhash: u32,
    fofs: usize,
    fmask: usize,
    hash: Option<u32>,
}

impl<'symtab> SymbolInfo<'symtab> {
    #[allow(unused_variables)]
    pub(crate) fn from_str(
        name: &'symtab str,
        version: Option<&'symtab str>,
    ) -> Self {
        SymbolInfo {
            name,
            cname: None,
            #[cfg(feature = "version")]
            version: version.map(crate::version::SymbolVersion::new),
        }
    }

    /// Gets the name of the symbol.
    #[inline]
    pub fn name(&self) -> &str {
        self.name
    }

    /// Gets the C-style name of the symbol.
    #[inline]
    pub fn cname(&self) -> Option<&CStr> {
        self.cname
    }

    #[inline]
    pub fn precompute(&self) -> PreCompute {
        let gnuhash = ElfGnuHash::hash(self.name.as_bytes());
        PreCompute {
            gnuhash,
            fofs: gnuhash as usize / usize::BITS as usize,
            fmask: 1 << (gnuhash % (8 * size_of::<usize>() as u32)),
            hash: None,
        }
    }
}

impl SymbolTable {
    pub(crate) fn new(dynamic: &ElfDynamic) -> Self {
        let hashtab = match dynamic.hashtab {
            ElfDynamicHashTab::Gnu(off) => {
                SymbolTableHashTable::Gnu(ElfGnuHash::parse(off as *const u8))
            }
            ElfDynamicHashTab::Elf(off) => {
                SymbolTableHashTable::Elf(ElfHash::parse(off as *const u8))
            }
        };
        let symtab = dynamic.symtab as *const ElfSymbol;
        let strtab = ElfStringTable::new(dynamic.strtab as *const u8);
        #[cfg(feature = "version")]
        let version = super::version::ELFVersion::new(
            dynamic.version_idx,
            dynamic.verneed,
            dynamic.verdef,
            &strtab,
        );
        SymbolTable {
            hashtab,
            symtab,
            strtab,
            #[cfg(feature = "version")]
            version,
        }
    }

    pub(crate) fn strtab(&self) -> &ElfStringTable {
        &self.strtab
    }

    /// Use the symbol specific information to get the symbol in the symbol table
    pub fn lookup(&self, symbol: &SymbolInfo, precompute: &mut PreCompute) -> Option<&ElfSymbol> {
        match &self.hashtab {
            SymbolTableHashTable::Gnu(hashtab) => {
                let hash = precompute.gnuhash;
                let fofs = precompute.fofs;
                let fmask = precompute.fmask;
                let bloom_idx = fofs & (hashtab.header.nbloom - 1) as usize;
                let filter = unsafe { hashtab.blooms.add(bloom_idx).read() };
                if filter & fmask == 0 {
                    return None;
                }
                let filter2 =
                    filter >> ((hash >> hashtab.header.nshift) as usize % usize::BITS as usize);
                if filter2 & 1 == 0 {
                    return None;
                }
                let table_start_idx = hashtab.header.symbias as usize;
                let chain_start_idx = unsafe {
                    hashtab
                        .buckets
                        .add((hash as usize) % hashtab.header.nbucket as usize)
                        .read()
                } as usize;
                if chain_start_idx == 0 {
                    return None;
                }
                let mut dynsym_idx = chain_start_idx;
                let mut cur_chain = unsafe { hashtab.chains.add(dynsym_idx - table_start_idx) };
                let mut cur_symbol_ptr = unsafe { self.symtab.add(dynsym_idx) };
                loop {
                    let chain_hash = unsafe { cur_chain.read() };
                    if hash | 1 == chain_hash | 1 {
                        let cur_symbol = unsafe { &*cur_symbol_ptr };
                        let sym_name = self.strtab.get_str(cur_symbol.st_name());
                        #[cfg(feature = "version")]
                        if sym_name == symbol.name && self.check_match(dynsym_idx, &symbol.version)
                        {
                            return Some(cur_symbol);
                        }
                        #[cfg(not(feature = "version"))]
                        if sym_name == symbol.name {
                            return Some(cur_symbol);
                        }
                    }
                    if chain_hash & 1 != 0 {
                        break;
                    }
                    cur_chain = unsafe { cur_chain.add(1) };
                    cur_symbol_ptr = unsafe { cur_symbol_ptr.add(1) };
                    dynsym_idx += 1;
                }
            }
            SymbolTableHashTable::Elf(hashtab) => {
                let hash = if let Some(hash) = precompute.hash {
                    hash
                } else {
                    let hash = ElfHash::hash(symbol.name.as_bytes());
                    precompute.hash = Some(hash);
                    hash
                };
                let bucket_idx = (hash as usize) % hashtab.header.nbucket as usize;
                let bucket_ptr = unsafe { hashtab.buckets.add(bucket_idx) };
                let mut chain_idx = unsafe { bucket_ptr.read() as usize };
                loop {
                    if chain_idx == 0 {
                        return None;
                    }
                    let chain_ptr = unsafe { hashtab.chains.add(chain_idx) };
                    let cur_symbol = unsafe { &*self.symtab.add(chain_idx) };
                    let sym_name = self.strtab.get_str(cur_symbol.st_name());
                    #[cfg(feature = "version")]
                    if sym_name == symbol.name && self.check_match(chain_idx, &symbol.version) {
                        return Some(cur_symbol);
                    }
                    #[cfg(not(feature = "version"))]
                    if sym_name == symbol.name {
                        return Some(cur_symbol);
                    }
                    chain_idx = unsafe { chain_ptr.read() as usize };
                }
            }
        }
        None
    }

    /// Use the symbol specific information to get the symbol which can be used for relocation in the symbol table
    #[inline]
    pub fn lookup_filter(
        &self,
        symbol: &SymbolInfo,
        precompute: &mut PreCompute,
    ) -> Option<&ElfSymbol> {
        if let Some(sym) = self.lookup(symbol, precompute) {
            if !sym.is_undef() && sym.is_ok_bind() && sym.is_ok_type() {
                return Some(sym);
            }
        }
        None
    }

    /// Use the symbol index to get the symbols in the symbol table.
    pub fn symbol_idx<'symtab>(
        &'symtab self,
        idx: usize,
    ) -> (&'symtab ElfSymbol, SymbolInfo<'symtab>) {
        let symbol = unsafe { &*self.symtab.add(idx) };
        let cname = self.strtab.get_cstr(symbol.st_name());
        let name = ElfStringTable::convert_cstr(cname);
        (
            symbol,
            SymbolInfo {
                name,
                cname: Some(cname),
                #[cfg(feature = "version")]
                version: self.get_requirement(idx),
            },
        )
    }

    #[inline]
    pub fn count_syms(&self) -> usize {
        match &self.hashtab {
            SymbolTableHashTable::Gnu(hashtab) => hashtab.count_syms(),
            SymbolTableHashTable::Elf(hashtab) => hashtab.count_syms(),
        }
    }
}
