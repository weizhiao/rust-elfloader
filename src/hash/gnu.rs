use crate::{
    arch::ElfSymbol,
    hash::{ElfHashTable, PreCompute},
    symbol::{SymbolInfo, SymbolTable},
};

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
    fn hash(name: &[u8]) -> u64 {
        let mut hash = 5381u32;
        for byte in name {
            hash = hash.wrapping_mul(33).wrapping_add(u32::from(*byte));
        }
        hash as u64
    }

    fn count_syms(&self) -> usize {
        let mut nsym = 0;
        for i in 0..self.header.nbucket as usize {
            nsym = nsym.max(unsafe { self.buckets.add(i).read() as usize });
        }
        if nsym > 0 {
            unsafe {
                let mut val = self.chains.add(nsym - self.header.symbias as usize);
                while val.read() & 1 == 0 {
                    nsym += 1;
                    val = val.add(1);
                }
            }
        }
        nsym + 1
    }

    fn lookup<'sym>(
        table: &'sym SymbolTable,
        symbol: &SymbolInfo,
        precompute: &mut PreCompute,
    ) -> Option<&'sym ElfSymbol> {
        let hash = precompute.gnuhash;
        let fofs = precompute.fofs;
        let fmask = precompute.fmask;
        let hashtab = table.hashtab.into_gnuhash().unwrap();
        let bloom_idx = fofs & (hashtab.header.nbloom - 1) as usize;
        let filter = unsafe { hashtab.blooms.add(bloom_idx).read() };
        if filter & fmask == 0 {
            return None;
        }
        let filter2 = filter >> ((hash >> hashtab.header.nshift) as usize % usize::BITS as usize);
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
        let mut cur_symbol_ptr = unsafe { table.symtab.add(dynsym_idx) };
        loop {
            let chain_hash = unsafe { cur_chain.read() };
            if hash | 1 == chain_hash | 1 {
                let cur_symbol = unsafe { &*cur_symbol_ptr };
                let sym_name = table.strtab.get_str(cur_symbol.st_name());
                #[cfg(feature = "version")]
                if sym_name == symbol.name && hashtab.check_match(dynsym_idx, &symbol.version) {
                    return Some(cur_symbol);
                }
                #[cfg(not(feature = "version"))]
                if sym_name == symbol.name() {
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
}
