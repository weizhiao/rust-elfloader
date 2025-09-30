use crate::{
    arch::{ElfShdr, ElfSymbol},
    dynamic::ElfDynamic,
    hash::HashTable,
};
use core::ffi::CStr;

pub use crate::hash::PreCompute;

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

/// Symbol table of elf file.
pub struct SymbolTable {
    /// .gnu.hash / .hash
    pub(crate) hashtab: HashTable,
    /// .dynsym
    pub(crate) symtab: *const ElfSymbol,
    /// .dynstr
    pub(crate) strtab: ElfStringTable,
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
    #[allow(unused_variables)]
    pub(crate) fn from_str(name: &'symtab str, version: Option<&'symtab str>) -> Self {
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
}

impl SymbolTable {
    pub(crate) fn from_dynamic(dynamic: &ElfDynamic) -> Self {
        let hashtab = HashTable::from_dynamic(dynamic);
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

    pub(crate) fn from_shdrs(base: usize, symtab: &ElfShdr, shdrs: &[ElfShdr]) -> Self {
        let strtab_shdr = &shdrs[symtab.sh_link as usize];
        let strtab = ElfStringTable::new(strtab_shdr.sh_addr as *const u8);
        let hashtab = HashTable::from_shdr(base, symtab, &strtab, shdrs);
        Self {
            hashtab,
            symtab: symtab.sh_addr as *const ElfSymbol,
            strtab,
            #[cfg(feature = "version")]
            None,
        }
    }

    pub(crate) fn strtab(&self) -> &ElfStringTable {
        &self.strtab
    }

    /// Use the symbol specific information to get the symbol in the symbol table
    pub fn lookup(&self, symbol: &SymbolInfo, precompute: &mut PreCompute) -> Option<&ElfSymbol> {
        self.hashtab.lookup(self, symbol, precompute)
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
        self.hashtab.count_syms()
    }
}
