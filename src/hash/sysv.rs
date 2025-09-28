use crate::{
    arch::ElfSymbol,
    hash::{ElfHashTable, PreCompute},
    symbol::{SymbolInfo, SymbolTable},
};

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
    fn hash(name: &[u8]) -> u64 {
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
        hash as u64
    }

    #[inline]
    fn count_syms(&self) -> usize {
        self.header.nchain as usize
    }

    fn lookup<'sym>(
        table: &'sym SymbolTable,
        symbol: &SymbolInfo,
        precompute: &mut PreCompute,
    ) -> Option<&'sym ElfSymbol> {
        let hash = if let Some(hash) = precompute.hash {
            hash
        } else {
            let hash = ElfHash::hash(symbol.name().as_bytes()) as u32;
            precompute.hash = Some(hash);
            hash
        };
        let hashtab = table.hashtab.into_elfhash().unwrap();
        let bucket_idx = (hash as usize) % hashtab.header.nbucket as usize;
        let bucket_ptr = unsafe { hashtab.buckets.add(bucket_idx) };
        let mut chain_idx = unsafe { bucket_ptr.read() as usize };
        loop {
            if chain_idx == 0 {
                return None;
            }
            let chain_ptr = unsafe { hashtab.chains.add(chain_idx) };
            let cur_symbol = unsafe { &*table.symtab.add(chain_idx) };
            let sym_name = table.strtab.get_str(cur_symbol.st_name());
            #[cfg(feature = "version")]
            if sym_name == symbol.name && hashtab.check_match(chain_idx, &symbol.version) {
                return Some(cur_symbol);
            }
            #[cfg(not(feature = "version"))]
            if sym_name == symbol.name() {
                return Some(cur_symbol);
            }
            chain_idx = unsafe { chain_ptr.read() as usize };
        }
    }
}
