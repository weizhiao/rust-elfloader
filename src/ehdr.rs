use crate::{
    Result,
    arch::{E_CLASS, EHDR_SIZE, EM_ARCH, Ehdr},
    parse_ehdr_error,
};
use core::ops::Deref;
use elf::abi::{EI_CLASS, EI_VERSION, ELFMAGIC, ET_DYN, EV_CURRENT};

#[repr(transparent)]
pub struct ElfHeader {
    ehdr: Ehdr,
}

impl Clone for ElfHeader {
    fn clone(&self) -> Self {
        Self {
            ehdr: Ehdr {
                e_ident: self.e_ident,
                e_type: self.e_type,
                e_machine: self.e_machine,
                e_version: self.e_version,
                e_entry: self.e_entry,
                e_phoff: self.e_phoff,
                e_shoff: self.e_shoff,
                e_flags: self.e_flags,
                e_ehsize: self.e_ehsize,
                e_phentsize: self.e_phentsize,
                e_phnum: self.e_phnum,
                e_shentsize: self.e_shentsize,
                e_shnum: self.e_shnum,
                e_shstrndx: self.e_shstrndx,
            },
        }
    }
}

impl Deref for ElfHeader {
    type Target = Ehdr;

    fn deref(&self) -> &Self::Target {
        &self.ehdr
    }
}

impl ElfHeader {
    pub(crate) fn new(data: &[u8]) -> Result<&Self> {
        debug_assert!(data.len() >= EHDR_SIZE);
        let ehdr: &ElfHeader = unsafe { &*(data.as_ptr().cast()) };
        ehdr.vaildate()?;
        Ok(ehdr)
    }

    #[inline]
    pub fn is_dylib(&self) -> bool {
        self.ehdr.e_type == ET_DYN
    }

    pub(crate) fn vaildate(&self) -> Result<()> {
        if self.e_ident[0..4] != ELFMAGIC {
            return Err(parse_ehdr_error("invalid ELF magic"));
        }
        if self.e_ident[EI_CLASS] != E_CLASS {
            return Err(parse_ehdr_error("file class mismatch"));
        }
        if self.e_ident[EI_VERSION] != EV_CURRENT {
            return Err(parse_ehdr_error("invalid ELF version"));
        }
        if self.e_machine != EM_ARCH {
            return Err(parse_ehdr_error("file arch mismatch"));
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn e_phnum(&self) -> usize {
        self.ehdr.e_phnum as usize
    }

    #[inline]
    pub(crate) fn e_phentsize(&self) -> usize {
        self.ehdr.e_phentsize as usize
    }

    #[inline]
    pub(crate) fn e_phoff(&self) -> usize {
        self.ehdr.e_phoff as usize
    }

    #[inline]
    pub(crate) fn e_shoff(&self) -> usize {
        self.ehdr.e_shoff as usize
    }

    #[inline]
    pub(crate) fn e_shentsize(&self) -> usize {
        self.ehdr.e_shentsize as usize
    }

    #[inline]
    pub(crate) fn e_shnum(&self) -> usize {
        self.ehdr.e_shnum as usize
    }

    #[inline]
    pub(crate) fn phdr_range(&self) -> (usize, usize) {
        let phdrs_size = self.e_phentsize() * self.e_phnum();
        let phdr_start = self.e_phoff();
        let phdr_end = phdr_start + phdrs_size;
        (phdr_start, phdr_end)
    }

    #[inline]
    pub(crate) fn shdr_range(&self) -> (usize, usize) {
        let shdrs_size = self.e_shentsize() * self.e_shnum();
        let shdr_start = self.e_shoff();
        let shdr_end = shdr_start + shdrs_size;
        (shdr_start, shdr_end)
    }
}
