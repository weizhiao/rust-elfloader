use crate::elf::ElfPhdr;
use alloc::vec::Vec;

/// Internal representation of ELF program headers
#[derive(Clone)]
pub(crate) enum ElfPhdrs {
    /// Program headers mapped from memory
    Mmap(&'static [ElfPhdr]),

    /// Program headers stored in a vector
    Vec(Vec<ElfPhdr>),
}

impl ElfPhdrs {
    pub(crate) fn as_slice(&self) -> &[ElfPhdr] {
        match self {
            ElfPhdrs::Mmap(phdrs) => phdrs,
            ElfPhdrs::Vec(phdrs) => phdrs,
        }
    }
}
