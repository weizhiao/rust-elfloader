use crate::common::RelocEntry;
use elf::abi::*;
use object::write::RelocationFlags;

pub(crate) fn get_relocs_static() -> Vec<RelocEntry> {
    vec![
        // Absolute
        RelocEntry {
            offset: 0x10,
            symbol_name: crate::EXTERNAL_FUNC_NAME.to_string(),
            addend: 0x10,
            flags: RelocationFlags::Elf {
                r_type: R_X86_64_64,
            },
        },
        // PC-relative / GOTPCREL
        RelocEntry {
            offset: 0x18,
            symbol_name: crate::EXTERNAL_VAR_NAME.to_string(),
            addend: 0,
            flags: RelocationFlags::Elf {
                r_type: R_X86_64_GOTPCREL,
            },
        },
        // Use GOTPCREL for PIC-friendly PC-relative access
        RelocEntry {
            offset: 0x20,
            symbol_name: crate::EXTERNAL_FUNC_NAME.to_string(),
            addend: 0,
            flags: RelocationFlags::Elf {
                r_type: R_X86_64_GOTPCREL,
            },
        },
        // PLT/PC32 to cause PLT entry generation by linker
        RelocEntry {
            offset: 0x28,
            symbol_name: crate::EXTERNAL_FUNC_NAME.to_string(),
            addend: 0,
            flags: RelocationFlags::Elf {
                r_type: R_X86_64_PLT32,
            },
        },
        // Relative-like relocation to data section
        RelocEntry {
            offset: 0x30,
            symbol_name: "".to_string(),
            addend: 0x2000,
            flags: RelocationFlags::Elf {
                r_type: R_X86_64_64,
            },
        },
        // TLS-like relocation (using 64-bit absolute for now)
        RelocEntry {
            offset: 0x38,
            symbol_name: crate::EXTERNAL_VAR_NAME.to_string(),
            addend: 0,
            flags: RelocationFlags::Elf {
                r_type: R_X86_64_64,
            },
        },
    ]
}
