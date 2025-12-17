use crate::{common::RelocEntry, Arch};
use elf::abi::*;
use object::RelocationFlags;
use std::collections::HashMap;

pub mod aarch64;
pub mod arm;
pub mod riscv32;
pub mod riscv64;
pub mod x86_64;

/// Analysis result of relocation types for GOT and PLT allocation
#[derive(Clone, Debug)]
pub struct RelocationTypeInfo {
    /// Number of GOT slots needed for non-PLT relocations
    pub non_plt_got_slots: u64,
    /// Next available GOT index for PLT entries (after reserved and non-PLT slots)
    pub next_got_index: u64,
    /// PLT entry symbols that need dynamic relocation
    pub plt_symbols: Vec<String>,
}

/// Extract r_type from RelocationFlags
fn get_r_type(flags: RelocationFlags) -> u64 {
    match flags {
        RelocationFlags::Elf { r_type } => r_type as u64,
        _ => 0,
    }
}

/// Check if a relocation type uses a GOT entry for the given architecture
pub fn uses_got_entry(arch: Arch, flags: RelocationFlags) -> bool {
    let r_type = get_r_type(flags);
    match arch {
        Arch::X86_64 => {
            r_type == R_X86_64_GLOB_DAT as u64
                || r_type == R_X86_64_GOTPCREL as u64
                || r_type == R_X86_64_JUMP_SLOT as u64
        }
        Arch::Aarch64 => {
            r_type == R_AARCH64_GLOB_DAT as u64 || r_type == R_AARCH64_JUMP_SLOT as u64
        }
        Arch::Arm => r_type == R_ARM_GLOB_DAT as u64 || r_type == R_ARM_JUMP_SLOT as u64,
        Arch::Riscv64 => r_type == R_RISCV_JUMP_SLOT as u64,
        Arch::Riscv32 => r_type == R_RISCV_JUMP_SLOT as u64,
    }
}

/// Check if a relocation is a PLT-related type for the given architecture
pub fn is_plt_reloc(arch: Arch, flags: RelocationFlags) -> bool {
    let r_type = get_r_type(flags);
    match arch {
        Arch::X86_64 => r_type == R_X86_64_JUMP_SLOT as u64 || r_type == R_X86_64_PLT32 as u64,
        Arch::Aarch64 => r_type == R_AARCH64_JUMP_SLOT as u64,
        Arch::Arm => r_type == R_ARM_JUMP_SLOT as u64,
        Arch::Riscv64 => r_type == R_RISCV_JUMP_SLOT as u64,
        Arch::Riscv32 => r_type == R_RISCV_JUMP_SLOT as u64,
    }
}

/// Check if a relocation needs PLT processing (includes JUMP_SLOT, PLT32, and IRELATIVE)
pub fn needs_plt_processing(arch: Arch, r_type: u64) -> bool {
    match arch {
        Arch::X86_64 => {
            r_type == R_X86_64_PLT32 as u64
                || r_type == R_X86_64_JUMP_SLOT as u64
                || r_type == R_X86_64_IRELATIVE as u64
        }
        Arch::Aarch64 => {
            r_type == R_AARCH64_JUMP_SLOT as u64 || r_type == R_AARCH64_IRELATIVE as u64
        }
        Arch::Riscv64 | Arch::Riscv32 => {
            r_type == R_RISCV_JUMP_SLOT as u64 || r_type == R_RISCV_IRELATIVE as u64
        }
        Arch::Arm => r_type == R_ARM_JUMP_SLOT as u64 || r_type == R_ARM_IRELATIVE as u64,
    }
}

/// Check if a relocation is RELATIVE type using raw r_type value
pub fn is_relative_reloc_by_type(arch: Arch, r_type: u64) -> bool {
    match arch {
        Arch::X86_64 => r_type == R_X86_64_RELATIVE as u64,
        Arch::Aarch64 => r_type == R_AARCH64_RELATIVE as u64,
        Arch::Riscv64 | Arch::Riscv32 => r_type == R_RISCV_RELATIVE as u64,
        Arch::Arm => r_type == R_ARM_RELATIVE as u64,
    }
}

/// Check if a relocation is RELATIVE type (doesn't depend on symbols)
pub fn is_relative_reloc(arch: Arch, flags: RelocationFlags) -> bool {
    let r_type = get_r_type(flags);
    match arch {
        Arch::X86_64 => r_type == R_X86_64_RELATIVE as u64,
        Arch::Aarch64 => r_type == R_AARCH64_RELATIVE as u64,
        Arch::Riscv64 | Arch::Riscv32 => r_type == R_RISCV_RELATIVE as u64,
        Arch::Arm => r_type == R_ARM_RELATIVE as u64,
    }
}

/// Calculate addend value for a relocation based on type
pub fn calculate_addend(_arch: Arch, r_type: u64, reloc_offset: u64) -> i64 {
    match r_type {
        // Absolute 64-bit relocations
        r if r == R_X86_64_64 as u64 || r == R_AARCH64_ABS64 as u64 || r == R_RISCV_64 as u64 => {
            0x10
        }
        // Global data relocations
        r if r == R_X86_64_GLOB_DAT as u64 || r == R_AARCH64_GLOB_DAT as u64 => 0,
        // PLT jump slot relocations
        r if r == R_X86_64_JUMP_SLOT as u64
            || r == R_AARCH64_JUMP_SLOT as u64
            || r == R_RISCV_JUMP_SLOT as u64
            || r == R_ARM_JUMP_SLOT as u64 =>
        {
            0
        }
        // Relative relocations (load-time relocation, value = base + addend)
        r if r == R_X86_64_RELATIVE as u64
            || r == R_AARCH64_RELATIVE as u64
            || r == R_RISCV_RELATIVE as u64
            || r == R_ARM_RELATIVE as u64 =>
        {
            0x2000
        }
        // Indirect function relocations (resolver function pointers)
        // Now IFUNC resolver is in .text section at offset 0x20
        // .plt is at 0x1000, .text is at 0x2000
        // So IFUNC resolver is at 0x2000 + 0x20 = 0x2020
        r if r == R_X86_64_IRELATIVE as u64
            || r == R_AARCH64_IRELATIVE as u64
            || r == R_RISCV_IRELATIVE as u64
            || r == R_ARM_IRELATIVE as u64 =>
        {
            0x402020 // text_vaddr (0x402000) + 0x20
        }
        // Default: use relocation offset as addend
        _ => reloc_offset as i64,
    }
}

/// Analyze relocation types to determine GOT and PLT allocation requirements
pub fn analyze_relocation_types(arch: Arch, relocs: &[RelocEntry]) -> RelocationTypeInfo {
    let mut non_plt_got_slots = 0u64;
    let mut plt_symbols = Vec::new();
    let mut seen_symbols = std::collections::HashSet::new();

    for reloc in relocs {
        if is_plt_reloc(arch, reloc.flags) {
            // Collect unique PLT symbols
            if seen_symbols.insert(reloc.symbol_name.clone()) {
                plt_symbols.push(reloc.symbol_name.clone());
            }
        } else if uses_got_entry(arch, reloc.flags) {
            // Count non-PLT GOT slots (only once per symbol)
            if seen_symbols.insert(reloc.symbol_name.clone()) {
                non_plt_got_slots += 1;
            }
        }
    }

    // GOT[0-2] are reserved, then non-PLT GOT entries, then PLT entries
    let next_got_index = 3 + non_plt_got_slots;

    RelocationTypeInfo {
        non_plt_got_slots,
        next_got_index,
        plt_symbols,
    }
}

///
/// # Arguments
/// * `arch` - Target architecture
/// * `base_addr` - Base address for memory mapping (used for RIP-relative calculations)
/// * `relocs` - List of relocations to determine PLT entries needed
/// * `got_offset` - Offset of GOT table in data segment
/// * `next_got_index` - Next available GOT index for PLT entries
///
/// # Returns
/// (text_data, plt_entries_map)
pub fn generate_plt_for_arch(
    arch: Arch,
    base_addr: u64,
    relocs: &[RelocEntry],
    got_offset: u64,
    next_got_index: u64,
) -> (Vec<u8>, HashMap<String, u64>) {
    match arch {
        Arch::X86_64 => x86_64::generate_plt(base_addr, relocs, got_offset, next_got_index),
        Arch::Aarch64 => aarch64::generate_plt(base_addr, relocs, got_offset, next_got_index),
        Arch::Arm => arm::generate_plt(base_addr, relocs, got_offset, next_got_index),
        Arch::Riscv64 => riscv64::generate_plt(base_addr, relocs, got_offset, next_got_index),
        Arch::Riscv32 => riscv32::generate_plt(base_addr, relocs, got_offset, next_got_index),
    }
}

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
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME.to_string(),
            RelocationFlags::Elf { r_type: abs64 },
        ),
        // GOT / GLOB_DAT-like
        RelocEntry::new(
            crate::EXTERNAL_VAR_NAME.to_string(),
            RelocationFlags::Elf { r_type: glob_dat },
        ),
        // PC-relative / small offset (use abs64 as common placeholder)
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME.to_string(),
            RelocationFlags::Elf { r_type: abs64 },
        ),
        // PLT / JUMP_SLOT-like
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME.to_string(),
            RelocationFlags::Elf { r_type: jump_slot },
        ),
        // RELATIVE
        RelocEntry::new(
            "".to_string(), // RELATIVE relocations don't need a symbol
            RelocationFlags::Elf { r_type: relative },
        ),
        // DTPMOD - Temporarily commented out for testing (需要外部动态库定义TLS符号)
        // RelocEntry::new(
        //     crate::EXTERNAL_TLS_NAME.to_string(),
        //     RelocationFlags::Elf { r_type: dtpoff },
        // ),
        // IRELATIVE - resolver function at offset 0x20 in .text (movabs rax, 0x1000; ret)
        RelocEntry::new(
            "".to_string(), // IRELATIVE relocations don't need a symbol
            RelocationFlags::Elf { r_type: irelative },
        ),
        // COPY relocation is not typically used in dynamic libraries themselves,
        // only in executables that link against them. Comment out for now.
        // RelocEntry::new(
        //     crate::EXTERNAL_VAR_NAME.to_string(),
        //     RelocationFlags::Elf { r_type: copy },
        // ),
    ]
}
