use crate::common::RelocType;
use clap::ValueEnum;
use object::Architecture;
use object::elf::*;

pub mod aarch64;
pub mod arm;
pub mod loongarch64;
pub mod riscv32;
pub mod riscv64;
pub mod x86;
pub mod x86_64;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq)]
pub enum Arch {
    X86_64,
    X86,
    Aarch64,
    Riscv64,
    Riscv32,
    Arm,
    Loongarch64,
}

impl Arch {
    pub fn is_64(&self) -> bool {
        match self {
            Arch::X86_64 | Arch::Aarch64 | Arch::Riscv64 | Arch::Loongarch64 => true,
            Arch::X86 | Arch::Riscv32 | Arch::Arm => false,
        }
    }

    pub fn is_rela(&self) -> bool {
        match self {
            Arch::X86_64 | Arch::Aarch64 | Arch::Riscv64 | Arch::Riscv32 | Arch::Loongarch64 => {
                true
            }
            Arch::X86 | Arch::Arm => false,
        }
    }
}

impl From<Arch> for Architecture {
    fn from(arch: Arch) -> Self {
        match arch {
            Arch::X86_64 => Architecture::X86_64,
            Arch::X86 => Architecture::I386,
            Arch::Aarch64 => Architecture::Aarch64,
            Arch::Riscv64 => Architecture::Riscv64,
            Arch::Riscv32 => Architecture::Riscv32,
            Arch::Arm => Architecture::Arm,
            Arch::Loongarch64 => Architecture::LoongArch64,
        }
    }
}

impl RelocType {
    /// Check if a relocation is a PLT-related type for the given architecture
    pub(crate) fn is_plt_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_JUMP_SLOT,
            Arch::X86 => r_type == R_386_JMP_SLOT,
            Arch::Aarch64 => r_type == R_AARCH64_JUMP_SLOT,
            Arch::Arm => r_type == R_ARM_JUMP_SLOT,
            Arch::Riscv64 => r_type == R_RISCV_JUMP_SLOT,
            Arch::Riscv32 => r_type == R_RISCV_JUMP_SLOT,
            Arch::Loongarch64 => r_type == R_LARCH_JUMP_SLOT,
        }
    }

    pub(crate) fn is_irelative_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_IRELATIVE,
            Arch::X86 => r_type == R_386_IRELATIVE,
            Arch::Aarch64 => r_type == R_AARCH64_IRELATIVE,
            Arch::Arm => r_type == R_ARM_IRELATIVE,
            Arch::Riscv64 => r_type == R_RISCV_IRELATIVE,
            Arch::Riscv32 => r_type == R_RISCV_IRELATIVE,
            Arch::Loongarch64 => r_type == R_LARCH_IRELATIVE,
        }
    }

    /// Check if a relocation is RELATIVE type (doesn't depend on symbols)
    pub(crate) fn is_relative_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_RELATIVE,
            Arch::X86 => r_type == R_386_RELATIVE,
            Arch::Aarch64 => r_type == R_AARCH64_RELATIVE,
            Arch::Riscv64 | Arch::Riscv32 => r_type == R_RISCV_RELATIVE,
            Arch::Arm => r_type == R_ARM_RELATIVE,
            Arch::Loongarch64 => r_type == R_LARCH_RELATIVE,
        }
    }

    pub(crate) fn is_abs_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_64,
            Arch::X86 => r_type == R_386_32,
            Arch::Aarch64 => r_type == R_AARCH64_ABS64,
            Arch::Riscv64 => r_type == R_RISCV_64,
            Arch::Riscv32 => r_type == R_RISCV_32,
            Arch::Arm => r_type == R_ARM_ABS32,
            Arch::Loongarch64 => r_type == R_LARCH_64,
        }
    }

    pub(crate) fn is_glob_dat_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_GLOB_DAT,
            Arch::X86 => r_type == R_386_GLOB_DAT,
            Arch::Aarch64 => r_type == R_AARCH64_GLOB_DAT,
            Arch::Arm => r_type == R_ARM_GLOB_DAT,
            Arch::Riscv64 => r_type == R_RISCV_64,
            Arch::Riscv32 => r_type == R_RISCV_32,
            Arch::Loongarch64 => r_type == R_LARCH_64,
        }
    }

    pub(crate) fn is_copy_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => r_type == R_X86_64_COPY,
            Arch::X86 => r_type == R_386_COPY,
            Arch::Aarch64 => r_type == R_AARCH64_COPY,
            Arch::Arm => r_type == R_ARM_COPY,
            Arch::Riscv64 | Arch::Riscv32 => r_type == R_RISCV_COPY,
            Arch::Loongarch64 => r_type == R_LARCH_COPY,
        }
    }

    pub(crate) fn is_tls_reloc(&self, arch: Arch) -> bool {
        let r_type = self.as_u32();
        match arch {
            Arch::X86_64 => {
                r_type == R_X86_64_DTPMOD64
                    || r_type == R_X86_64_DTPOFF64
                    || r_type == R_X86_64_TPOFF64
            }
            Arch::X86 => {
                r_type == R_386_TLS_DTPMOD32
                    || r_type == R_386_TLS_DTPOFF32
                    || r_type == R_386_TLS_TPOFF
            }
            Arch::Aarch64 => {
                r_type == R_AARCH64_TLS_DTPMOD
                    || r_type == R_AARCH64_TLS_DTPREL
                    || r_type == R_AARCH64_TLS_TPREL
            }
            Arch::Arm => {
                r_type == R_ARM_TLS_DTPMOD32
                    || r_type == R_ARM_TLS_DTPOFF32
                    || r_type == R_ARM_TLS_TPOFF32
            }
            Arch::Riscv64 | Arch::Riscv32 => {
                r_type == R_RISCV_TLS_DTPMOD64
                    || r_type == R_RISCV_TLS_DTPREL64
                    || r_type == R_RISCV_TLS_TPREL64
            }
            Arch::Loongarch64 => {
                r_type == R_LARCH_TLS_DTPMOD64
                    || r_type == R_LARCH_TLS_DTPREL64
                    || r_type == R_LARCH_TLS_TPREL64
            }
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
    if r_type.is_glob_dat_reloc(arch) {
        0
    } else if r_type.is_abs_reloc(arch) {
        (sym_value + 0x10) as i64
    } else if r_type.is_plt_reloc(arch) {
        0
    } else if r_type.is_relative_reloc(arch) {
        if sym_value != 0 {
            sym_value as i64
        } else {
            data_vaddr as i64
        }
    } else if r_type.is_copy_reloc(arch) {
        0
    } else if r_type.is_tls_reloc(arch) {
        0
    } else {
        reloc_offset as i64
    }
}

pub fn generate_helper_code(arch: Arch) -> Vec<u8> {
    match arch {
        Arch::X86_64 => x86_64::generate_helper_code(),
        Arch::X86 => x86::generate_helper_code(),
        Arch::Aarch64 => aarch64::generate_helper_code(),
        Arch::Arm => arm::generate_helper_code(),
        Arch::Riscv64 => riscv64::generate_helper_code(),
        Arch::Riscv32 => riscv32::generate_helper_code(),
        Arch::Loongarch64 => loongarch64::generate_helper_code(),
    }
}

pub fn patch_helper(
    arch: Arch,
    text_data: &mut [u8],
    helper_text_off: usize,
    helper_vaddr: u64,
    target_plt_vaddr: u64,
    got_vaddr: u64,
) {
    match arch {
        Arch::X86_64 => {
            x86_64::patch_helper(text_data, helper_text_off, helper_vaddr, target_plt_vaddr)
        }
        Arch::X86 => x86::patch_helper(
            text_data,
            helper_text_off,
            helper_vaddr,
            target_plt_vaddr,
            got_vaddr,
        ),
        Arch::Aarch64 => {
            aarch64::patch_helper(text_data, helper_text_off, helper_vaddr, target_plt_vaddr)
        }
        Arch::Arm => arm::patch_helper(text_data, helper_text_off, helper_vaddr, target_plt_vaddr),
        Arch::Riscv64 => {
            riscv64::patch_helper(text_data, helper_text_off, helper_vaddr, target_plt_vaddr)
        }
        Arch::Riscv32 => {
            riscv32::patch_helper(text_data, helper_text_off, helper_vaddr, target_plt_vaddr)
        }
        Arch::Loongarch64 => {
            loongarch64::patch_helper(text_data, helper_text_off, helper_vaddr, target_plt_vaddr)
        }
    }
}

pub fn get_ifunc_resolver_code(arch: Arch) -> Vec<u8> {
    match arch {
        Arch::X86_64 => x86_64::get_ifunc_resolver_code(),
        Arch::X86 => x86::get_ifunc_resolver_code(),
        Arch::Aarch64 => aarch64::get_ifunc_resolver_code(),
        Arch::Arm => arm::get_ifunc_resolver_code(),
        Arch::Riscv64 => riscv64::get_ifunc_resolver_code(),
        Arch::Riscv32 => riscv32::get_ifunc_resolver_code(),
        Arch::Loongarch64 => loongarch64::get_ifunc_resolver_code(),
    }
}

pub fn patch_ifunc_resolver(
    arch: Arch,
    text_data: &mut [u8],
    offset: usize,
    resolver_vaddr: u64,
    target_vaddr: u64,
) {
    match arch {
        Arch::X86_64 => {
            x86_64::patch_ifunc_resolver(text_data, offset, resolver_vaddr, target_vaddr)
        }
        Arch::X86 => x86::patch_ifunc_resolver(text_data, offset, resolver_vaddr, target_vaddr),
        Arch::Aarch64 => {
            aarch64::patch_ifunc_resolver(text_data, offset, resolver_vaddr, target_vaddr)
        }
        Arch::Arm => arm::patch_ifunc_resolver(text_data, offset, resolver_vaddr, target_vaddr),
        Arch::Riscv64 => {
            riscv64::patch_ifunc_resolver(text_data, offset, resolver_vaddr, target_vaddr)
        }
        Arch::Riscv32 => {
            riscv32::patch_ifunc_resolver(text_data, offset, resolver_vaddr, target_vaddr)
        }
        Arch::Loongarch64 => {
            loongarch64::patch_ifunc_resolver(text_data, offset, resolver_vaddr, target_vaddr)
        }
    }
}

pub fn generate_plt0_code(arch: Arch) -> Vec<u8> {
    match arch {
        Arch::X86_64 => x86_64::generate_plt0_code(),
        Arch::X86 => x86::generate_plt0_code(),
        Arch::Aarch64 => aarch64::generate_plt0_code(),
        Arch::Arm => arm::generate_plt0_code(),
        Arch::Riscv64 => riscv64::generate_plt0_code(),
        Arch::Riscv32 => riscv32::generate_plt0_code(),
        Arch::Loongarch64 => loongarch64::generate_plt0_code(),
    }
}

pub fn generate_plt_entry_code(arch: Arch, reloc_idx: u32, plt_entry_offset: u64) -> Vec<u8> {
    match arch {
        Arch::X86_64 => x86_64::generate_plt_entry_code(reloc_idx, plt_entry_offset),
        Arch::X86 => x86::generate_plt_entry_code(reloc_idx, plt_entry_offset),
        Arch::Aarch64 => aarch64::generate_plt_entry_code(),
        Arch::Arm => arm::generate_plt_entry_code(),
        Arch::Riscv64 => riscv64::generate_plt_entry_code(),
        Arch::Riscv32 => riscv32::generate_plt_entry_code(reloc_idx, plt_entry_offset),
        Arch::Loongarch64 => loongarch64::generate_plt_entry_code(reloc_idx, plt_entry_offset),
    }
}

pub fn patch_plt0(
    arch: Arch,
    plt_data: &mut [u8],
    plt0_off: usize,
    plt0_vaddr: u64,
    got_plt_vaddr: u64,
) {
    match arch {
        Arch::X86_64 => x86_64::patch_plt0(plt_data, plt0_off, plt0_vaddr, got_plt_vaddr),
        Arch::X86 => {}
        Arch::Aarch64 => aarch64::patch_plt0(plt_data, plt0_off, plt0_vaddr, got_plt_vaddr),
        Arch::Arm => arm::patch_plt0(plt_data, plt0_off, plt0_vaddr, got_plt_vaddr),
        Arch::Riscv64 => riscv64::patch_plt0(plt_data, plt0_off, plt0_vaddr, got_plt_vaddr),
        Arch::Riscv32 => riscv32::patch_plt0(plt_data, plt0_off, plt0_vaddr, got_plt_vaddr),
        Arch::Loongarch64 => loongarch64::patch_plt0(plt_data, plt0_off, plt0_vaddr, got_plt_vaddr),
    }
}

pub fn patch_plt_entry(
    arch: Arch,
    plt_data: &mut [u8],
    plt_entry_off: usize,
    plt_entry_vaddr: u64,
    target_got_vaddr: u64,
    got_vaddr: u64,
) {
    match arch {
        Arch::X86_64 => {
            x86_64::patch_plt_entry(plt_data, plt_entry_off, plt_entry_vaddr, target_got_vaddr)
        }
        Arch::X86 => x86::patch_plt_entry(plt_data, plt_entry_off, target_got_vaddr, got_vaddr),
        Arch::Aarch64 => {
            aarch64::patch_plt_entry(plt_data, plt_entry_off, plt_entry_vaddr, target_got_vaddr)
        }
        Arch::Arm => {
            arm::patch_plt_entry(plt_data, plt_entry_off, plt_entry_vaddr, target_got_vaddr)
        }
        Arch::Riscv64 => {
            riscv64::patch_plt_entry(plt_data, plt_entry_off, plt_entry_vaddr, target_got_vaddr)
        }
        Arch::Riscv32 => {
            riscv32::patch_plt_entry(plt_data, plt_entry_off, plt_entry_vaddr, target_got_vaddr)
        }
        Arch::Loongarch64 => {
            loongarch64::patch_plt_entry(plt_data, plt_entry_off, plt_entry_vaddr, target_got_vaddr)
        }
    }
}

pub fn get_got_plt_init_value(arch: Arch, plt_vaddr: u64, plt_entry_off: u64) -> u64 {
    // The initial value to be placed in the GOT.PLT entrys points to the instruction
    // in the PLT entry that jumps to the resolver code. This varies by architecture.
    match arch {
        Arch::X86_64 | Arch::X86 => plt_vaddr + plt_entry_off + 6,
        Arch::Aarch64 | Arch::Riscv64 | Arch::Arm => plt_vaddr,
        Arch::Riscv32 | Arch::Loongarch64 => plt_vaddr + plt_entry_off + 16,
    }
}

pub fn get_plt0_size(arch: Arch) -> u64 {
    match arch {
        Arch::X86_64 | Arch::X86 => 16,
        Arch::Arm => 20,
        Arch::Aarch64 | Arch::Riscv64 | Arch::Riscv32 | Arch::Loongarch64 => 32,
    }
}

pub fn get_plt_entry_size(arch: Arch) -> u64 {
    match arch {
        Arch::Arm => 12,
        Arch::X86_64 | Arch::X86 | Arch::Aarch64 | Arch::Riscv64 => 16,
        Arch::Riscv32 | Arch::Loongarch64 => 32,
    }
}
