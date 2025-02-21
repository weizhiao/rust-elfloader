use crate::{arch::ElfSymbol, dynamic::ElfDynamic};
use core::ffi::CStr;

#[derive(Clone)]
struct ElfGnuHash {
    nbucket: u32,
    table_start_idx: u32,
    nshift: u32,
    blooms: &'static [usize],
    buckets: *const u32,
    chains: *const u32,
}

impl ElfGnuHash {
    #[inline]
    pub(crate) fn parse(ptr: *const u8) -> ElfGnuHash {
        struct Reader {
            ptr: *const u8,
        }

        impl Reader {
            #[inline]
            fn new(ptr: *const u8) -> Reader {
                Reader { ptr }
            }

            #[inline]
            fn read<T>(&mut self) -> T {
                unsafe {
                    let value = self.ptr.cast::<T>().read();
                    self.ptr = self.ptr.add(core::mem::size_of::<T>());
                    value
                }
            }

            #[inline]
            //字节为单位
            fn add(&mut self, count: usize) {
                self.ptr = unsafe { self.ptr.add(count) };
            }

            #[inline]
            fn as_ptr(&self) -> *const u8 {
                self.ptr
            }
        }

        let mut reader = Reader::new(ptr);

        let nbucket: u32 = reader.read();
        let table_start_idx: u32 = reader.read();
        let nbloom: u32 = reader.read();
        let nshift: u32 = reader.read();
        let blooms_ptr = reader.as_ptr() as *const usize;
        let blooms = unsafe { core::slice::from_raw_parts(blooms_ptr, nbloom as _) };
        let bloom_size = nbloom as usize * core::mem::size_of::<usize>();
        reader.add(bloom_size);
        let buckets = reader.as_ptr() as _;
        reader.add(nbucket as usize * core::mem::size_of::<u32>());
        let chains = reader.as_ptr() as _;
        ElfGnuHash {
            nbucket,
            blooms,
            nshift,
            table_start_idx,
            buckets,
            chains,
        }
    }

    #[inline]
    pub(crate) fn gnu_hash(name: &[u8]) -> u32 {
        let mut hash = 5381u32;
        for byte in name {
            hash = hash.wrapping_mul(33).wrapping_add(u32::from(*byte));
        }
        hash
    }

    fn count_syms(&self) -> usize {
        let mut nsym = 0;
        for i in 0..self.nbucket as usize {
            nsym = nsym.max(unsafe { self.buckets.add(i).read() as usize });
        }
        if nsym > 0 {
            unsafe {
                let mut hashval = self.chains.add(nsym - self.table_start_idx as usize + 1);
                while hashval.read() & 1 == 0 {
                    nsym += 1;
                    hashval = hashval.add(1);
                }
            }
        }
        nsym + 1
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
    pub(crate) fn convert_cstr(s: &CStr) -> &str {
        unsafe { core::str::from_utf8_unchecked(s.to_bytes()) }
    }

    #[inline]
    pub(crate) fn get_str(&self, offset: usize) -> &'static str {
        Self::convert_cstr(self.get_cstr(offset))
    }
}

/// Symbol table of elf file.
pub struct SymbolTable {
    /// .gnu.hash
    hashtab: ElfGnuHash,
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

impl<'symtab> SymbolInfo<'symtab> {
    pub(crate) const fn from_str(name: &'symtab str) -> Self {
        SymbolInfo {
            name,
            cname: None,
            #[cfg(feature = "version")]
            version: None,
        }
    }

    #[cfg(feature = "version")]
    pub(crate) fn new_with_version(name: &'symtab str, version: &'symtab str) -> Self {
        SymbolInfo {
            name,
            cname: None,
            version: Some(crate::version::SymbolVersion::new(version)),
        }
    }

    /// Gets the name of the symbol.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the C-style name of the symbol.
    #[inline]
    pub fn cname(&self) -> Option<&CStr> {
        self.cname
    }
}

impl SymbolTable {
    pub(crate) fn new(dynamic: &ElfDynamic) -> Self {
        let hashtab = ElfGnuHash::parse(dynamic.hashtab as *const u8);
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
    pub fn lookup(&self, symbol: &SymbolInfo) -> Option<&ElfSymbol> {
        let hash = ElfGnuHash::gnu_hash(symbol.name.as_bytes());
        let bloom_width: u32 = 8 * size_of::<usize>() as u32;
        let bloom_idx = (hash / (bloom_width)) as usize % self.hashtab.blooms.len();
        let filter = self.hashtab.blooms[bloom_idx] as u64;
        if filter & (1 << (hash % bloom_width)) == 0 {
            return None;
        }
        let hash2 = hash >> self.hashtab.nshift;
        if filter & (1 << (hash2 % bloom_width)) == 0 {
            return None;
        }
        let table_start_idx = self.hashtab.table_start_idx as usize;
        let chain_start_idx = unsafe {
            self.hashtab
                .buckets
                .add((hash as usize) % self.hashtab.nbucket as usize)
                .read()
        } as usize;
        if chain_start_idx == 0 {
            return None;
        }
        let mut dynsym_idx = chain_start_idx;
        let mut cur_chain = unsafe { self.hashtab.chains.add(dynsym_idx - table_start_idx) };
        let mut cur_symbol_ptr = unsafe { self.symtab.add(dynsym_idx) };
        loop {
            let chain_hash = unsafe { cur_chain.read() };
            if hash | 1 == chain_hash | 1 {
                let cur_symbol = unsafe { &*cur_symbol_ptr };
                let sym_name = self.strtab.get_str(cur_symbol.st_name());
                #[cfg(feature = "version")]
                if sym_name == symbol.name && self.check_match(dynsym_idx, &symbol.version) {
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
        None
    }

    /// Use the symbol specific information to get the symbol which can be used for relocation in the symbol table
    #[inline]
    pub fn lookup_filter(&self, symbol: &SymbolInfo) -> Option<&ElfSymbol> {
        if let Some(sym) = self.lookup(symbol) {
            if !sym.is_undef() && sym.is_ok_bind() && sym.is_ok_type() {
                return Some(sym);
            }
        }
        None
    }

    #[inline]
    /// Use the symbol index to get the symbols in the symbol table.
    pub fn symbol_idx<'symtab>(
        &'symtab self,
        idx: usize,
    ) -> (&'symtab ElfSymbol, SymbolInfo<'symtab>) {
        let symbol = unsafe { &*self.symtab.add(idx) };
        let cname = self.strtab.get_cstr(symbol.st_name());
        let name = ElfStringTable::convert_cstr(&cname);
        (
            symbol,
            SymbolInfo {
                name,
                cname: Some(&cname),
                #[cfg(feature = "version")]
                version: self.get_requirement(idx),
            },
        )
    }

    #[inline]
    pub fn count_syms(&self) -> usize {
        self.hashtab.count_syms()
    }
}
