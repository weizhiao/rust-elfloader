use elf::abi::*;

pub(crate) const EM_ARCH: u16 = EM_X86_64;

#[allow(unused)]
pub const TLS_DTV_OFFSET: usize = 0;

pub(crate) const REL_RELATIVE: u32 = R_X86_64_RELATIVE;
pub(crate) const REL_GOT: u32 = R_X86_64_GLOB_DAT;
#[allow(unused)]
pub(crate) const REL_DTPMOD: u32 = R_X86_64_DTPMOD64;
pub(crate) const REL_SYMBOLIC: u32 = R_X86_64_64;
pub(crate) const REL_JUMP_SLOT: u32 = R_X86_64_JUMP_SLOT;
#[allow(unused)]
pub(crate) const REL_DTPOFF: u32 = R_X86_64_DTPOFF64;
