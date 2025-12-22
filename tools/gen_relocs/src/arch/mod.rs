use crate::common::RelocType;
use clap::ValueEnum;
use elf::abi::*;
use object::Architecture;

pub mod aarch64;
pub mod arm;
pub mod riscv32;
pub mod riscv64;
pub mod x86_64;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Riscv64,
    Riscv32,
    Arm,
}

impl Arch {
    pub fn is_64(&self) -> bool {
        match self {
            Arch::X86_64 | Arch::Aarch64 | Arch::Riscv64 => true,
            Arch::Riscv32 | Arch::Arm => false,
        }
    }

    pub fn is_rela(&self) -> bool {
        match self {
            Arch::X86_64 | Arch::Aarch64 | Arch::Riscv64 | Arch::Riscv32 => true,
            Arch::Arm => false,
        }
    }
}

impl From<Arch> for Architecture {
    fn from(arch: Arch) -> Self {
        match arch {
            Arch::X86_64 => Architecture::X86_64,
            Arch::Aarch64 => Architecture::Aarch64,
            Arch::Riscv64 => Architecture::Riscv64,
            Arch::Riscv32 => Architecture::Riscv32,
            Arch::Arm => Architecture::Arm,
        }
    }
}

impl RelocType {
    /// Check if a relocation is a PLT-related type for the given architecture
    pub(crate) fn is_plt_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_JUMP_SLOT,
            Arch::Aarch64 => r_type == R_AARCH64_JUMP_SLOT,
            Arch::Arm => r_type == R_ARM_JUMP_SLOT,
            Arch::Riscv64 => r_type == R_RISCV_JUMP_SLOT,
            Arch::Riscv32 => r_type == R_RISCV_JUMP_SLOT,
        }
    }

    pub(crate) fn is_irelative_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_IRELATIVE,
            Arch::Aarch64 => r_type == R_AARCH64_IRELATIVE,
            Arch::Arm => r_type == R_ARM_IRELATIVE,
            Arch::Riscv64 => r_type == R_RISCV_IRELATIVE,
            Arch::Riscv32 => r_type == R_RISCV_IRELATIVE,
        }
    }

    /// Check if a relocation is RELATIVE type (doesn't depend on symbols)
    pub(crate) fn is_relative_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_RELATIVE,
            Arch::Aarch64 => r_type == R_AARCH64_RELATIVE,
            Arch::Riscv64 | Arch::Riscv32 => r_type == R_RISCV_RELATIVE,
            Arch::Arm => r_type == R_ARM_RELATIVE,
        }
    }

    pub(crate) fn is_got_reloc(&self, arch: Arch) -> bool {
        self.is_glob_dat_reloc(arch) || self.is_abs_reloc(arch)
    }

    pub(crate) fn is_abs_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_64,
            Arch::Aarch64 => r_type == R_AARCH64_ABS64,
            Arch::Riscv64 => r_type == R_RISCV_64,
            Arch::Riscv32 => r_type == R_RISCV_32,
            Arch::Arm => r_type == R_ARM_ABS32,
        }
    }

    pub(crate) fn is_glob_dat_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_GLOB_DAT,
            Arch::Aarch64 => r_type == R_AARCH64_GLOB_DAT,
            Arch::Arm => r_type == R_ARM_GLOB_DAT,
            _ => false,
        }
    }
}

/// Calculate addend value for a relocation based on type and section virtual addresses
pub fn calculate_addend(
    arch: Arch,
    r_type: RelocType,
    reloc_offset: u64,
    data_vaddr: u64,
    sym_value: u64,
) -> i64 {
    if r_type.is_abs_reloc(arch) {
        (sym_value + 0x10) as i64
    } else if r_type.is_glob_dat_reloc(arch) {
        0
    } else if r_type.is_plt_reloc(arch) {
        0
    } else if r_type.is_relative_reloc(arch) {
        if sym_value != 0 {
            sym_value as i64
        } else {
            data_vaddr as i64
        }
    } else if r_type.is_irelative_reloc(arch) {
        sym_value as i64
    } else {
        reloc_offset as i64
    }
}

pub fn generate_helper_code(arch: Arch) -> Vec<u8> {
    match arch {
        Arch::X86_64 => x86_64::generate_helper_code(),
        _ => todo!("Architecture {:?} not supported", arch),
    }
}

pub fn refill_helper(
    arch: Arch,
    text_data: &mut [u8],
    helper_text_off: usize,
    helper_vaddr: u64,
    target_plt_vaddr: u64,
) {
    match arch {
        Arch::X86_64 => {
            x86_64::refill_helper(text_data, helper_text_off, helper_vaddr, target_plt_vaddr)
        }
        _ => todo!("Architecture {:?} not supported", arch),
    }
}

pub fn get_ifunc_resolver_code(arch: Arch) -> Vec<u8> {
    match arch {
        Arch::X86_64 => x86_64::get_ifunc_resolver_code(),
        _ => todo!("Architecture {:?} not supported", arch),
    }
}

pub fn refill_ifunc_resolver(arch: Arch, text_data: &mut [u8], offset: usize, plt_vaddr: u64) {
    match arch {
        Arch::X86_64 => x86_64::refill_ifunc_resolver(text_data, offset, plt_vaddr),
        _ => todo!("Architecture {:?} not supported", arch),
    }
}

pub fn generate_plt0_code(arch: Arch) -> Vec<u8> {
    match arch {
        Arch::X86_64 => x86_64::generate_plt0_code(),
        _ => todo!("Architecture {:?} not supported", arch),
    }
}

pub fn generate_plt_entry_code(
    arch: Arch,
    got_idx: u64,
    reloc_idx: u32,
    plt_entry_offset: u64,
) -> Vec<u8> {
    match arch {
        Arch::X86_64 => x86_64::generate_plt_entry_code(got_idx, reloc_idx, plt_entry_offset),
        _ => todo!("Architecture {:?} not supported", arch),
    }
}

pub fn refill_plt0(
    arch: Arch,
    plt_data: &mut [u8],
    plt0_off: usize,
    plt0_vaddr: u64,
    got_vaddr: u64,
) {
    match arch {
        Arch::X86_64 => x86_64::refill_plt0(plt_data, plt0_off, plt0_vaddr, got_vaddr),
        _ => todo!("Architecture {:?} not supported", arch),
    }
}

pub fn refill_plt_entry(
    arch: Arch,
    plt_data: &mut [u8],
    plt_entry_off: usize,
    plt_entry_vaddr: u64,
    target_got_vaddr: u64,
) {
    match arch {
        Arch::X86_64 => {
            x86_64::refill_plt_entry(plt_data, plt_entry_off, plt_entry_vaddr, target_got_vaddr)
        }
        _ => todo!("Architecture {:?} not supported", arch),
    }
}
