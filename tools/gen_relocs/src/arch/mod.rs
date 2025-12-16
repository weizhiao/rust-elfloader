use crate::{common::RelocEntry, Arch};
use elf::abi::*;
use object::RelocationFlags;

pub mod aarch64;
pub mod arm;
pub mod riscv32;
pub mod riscv64;
pub mod x86_64;

pub(crate) fn get_relocs_dynamic(arch: Arch) -> Vec<RelocEntry> {
    // choose arch-specific relocation type constants
    let (abs64, glob_dat, jump_slot, relative, _dtpoff, irelative, _copy) = match arch {
        Arch::X86_64 => (
            R_X86_64_64,
            R_X86_64_GLOB_DAT,
            R_X86_64_JUMP_SLOT,
            R_X86_64_RELATIVE,
            R_X86_64_DTPOFF64,
            R_X86_64_IRELATIVE,
            R_X86_64_COPY,
        ),
        Arch::Aarch64 => (
            R_AARCH64_ABS64,
            R_AARCH64_GLOB_DAT,
            R_AARCH64_JUMP_SLOT,
            R_AARCH64_RELATIVE,
            R_AARCH64_TLS_DTPREL,
            R_AARCH64_IRELATIVE,
            R_AARCH64_COPY,
        ),
        Arch::Riscv64 => (
            R_RISCV_64,
            R_RISCV_64, // RISCV uses same for GOT-like in many contexts; adjust later if needed
            R_RISCV_JUMP_SLOT,
            R_RISCV_RELATIVE,
            R_RISCV_TLS_DTPREL64,
            R_RISCV_IRELATIVE,
            R_RISCV_COPY,
        ),
        Arch::Riscv32 => (
            R_RISCV_32,
            R_RISCV_32,
            R_RISCV_JUMP_SLOT,
            R_RISCV_RELATIVE,
            R_RISCV_TLS_DTPREL32,
            R_RISCV_IRELATIVE,
            R_RISCV_COPY,
        ),
        Arch::Arm => (
            R_ARM_ABS32,
            R_ARM_GLOB_DAT,
            R_ARM_JUMP_SLOT,
            R_ARM_RELATIVE,
            R_ARM_TLS_DTPOFF32,
            R_ARM_IRELATIVE,
            R_ARM_COPY,
        ),
    };

    vec![
        // Absolute 64-bit
        RelocEntry {
            offset: crate::RELOC_OFF_ABS as u64,
            symbol_name: crate::EXTERNAL_FUNC_NAME.to_string(),
            addend: 0x10,
            flags: RelocationFlags::Elf { r_type: abs64 },
        },
        // GOT / GLOB_DAT-like
        RelocEntry {
            offset: crate::RELOC_OFF_GOT as u64,
            symbol_name: crate::EXTERNAL_VAR_NAME.to_string(),
            addend: 0,
            flags: RelocationFlags::Elf { r_type: glob_dat },
        },
        // PC-relative / small offset (use abs64 as common placeholder)
        RelocEntry {
            offset: crate::RELOC_OFF_PC_REL as u64,
            symbol_name: crate::EXTERNAL_FUNC_NAME.to_string(),
            addend: -4i64,
            flags: RelocationFlags::Elf { r_type: abs64 },
        },
        // PLT / JUMP_SLOT-like
        RelocEntry {
            offset: crate::RELOC_OFF_PLT as u64,
            symbol_name: crate::EXTERNAL_FUNC_NAME.to_string(),
            addend: 0,
            flags: RelocationFlags::Elf { r_type: jump_slot },
        },
        // RELATIVE
        RelocEntry {
            offset: crate::RELOC_OFF_RELATIVE as u64,
            symbol_name: "".to_string(), // RELATIVE relocations don't need a symbol
            addend: 0x2000,
            flags: RelocationFlags::Elf { r_type: relative },
        },
        // DTPMOD - Temporarily commented out for testing (需要外部动态库定义TLS符号)
        // RelocEntry {
        //     offset: crate::RELOC_OFF_DTPOFF as u64,
        //     symbol_name: crate::EXTERNAL_TLS_NAME.to_string(),
        //     addend: 0,
        //     flags: RelocationFlags::Elf { r_type: dtpoff },
        // },
        // IRELATIVE - resolver function at offset 0x20 in .text (movabs rax, 0x1000; ret)
        RelocEntry {
            offset: crate::RELOC_OFF_IRELATIVE as u64,
            symbol_name: "".to_string(), // IRELATIVE relocations don't need a symbol
            // Point to the resolver function in .text section
            // PLT[0] = 0x401000-0x40100F (16 bytes)
            // PLT[1] = 0x401010-0x40101F (16 bytes)
            // Resolver stub = 0x401020+ (movabs rax, 0x1000; ret)
            addend: 0x401020,
            flags: RelocationFlags::Elf { r_type: irelative },
        },
        // COPY relocation is not typically used in dynamic libraries themselves,
        // only in executables that link against them. Comment out for now.
        // RelocEntry {
        //     offset: crate::RELOC_OFF_COPY as u64,
        //     symbol_name: crate::EXTERNAL_VAR_NAME.to_string(),
        //     addend: 0,
        //     flags: RelocationFlags::Elf { r_type: copy },
        // },
    ]
}
