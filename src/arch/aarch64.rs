use elf::abi::*;

pub const EM_ARCH: u16 = EM_AARCH64;
#[allow(unused)]
pub const TLS_DTV_OFFSET: usize = 0;

pub const REL_RELATIVE: u32 = R_AARCH64_RELATIVE;
pub const REL_GOT: u32 = R_AARCH64_GLOB_DAT;
#[allow(unused)]
pub const REL_DTPMOD: u32 = R_AARCH64_TLS_DTPMOD;
pub const REL_SYMBOLIC: u32 = R_AARCH64_ABS64;
pub const REL_JUMP_SLOT: u32 = R_AARCH64_JUMP_SLOT;
#[allow(unused)]
pub const REL_DTPOFF: u32 = R_AARCH64_TLS_DTPREL;
