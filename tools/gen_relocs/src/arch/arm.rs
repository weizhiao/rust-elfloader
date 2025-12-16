use crate::common::RelocEntry;
use object::write::RelocationFlags;

pub fn get_relocs_static() -> Vec<RelocEntry> {
    vec![
        RelocEntry {
            offset: 0x10,
            symbol_name: crate::EXTERNAL_FUNC_NAME.to_string(),
            addend: 0x10,
            flags: RelocationFlags::Elf {
                r_type: elf::abi::R_ARM_ABS32,
            },
        },
        RelocEntry {
            offset: 0x18,
            symbol_name: crate::EXTERNAL_VAR_NAME.to_string(),
            addend: 0,
            flags: RelocationFlags::Elf {
                r_type: elf::abi::R_ARM_GLOB_DAT,
            },
        },
        RelocEntry {
            offset: 0x20,
            symbol_name: "".to_string(),
            addend: 0x20,
            flags: RelocationFlags::Elf {
                r_type: elf::abi::R_ARM_ABS32,
            },
        },
        RelocEntry {
            offset: 0x28,
            symbol_name: crate::EXTERNAL_FUNC_NAME.to_string(),
            addend: 0,
            flags: RelocationFlags::Elf {
                r_type: elf::abi::R_ARM_JUMP_SLOT,
            },
        },
        RelocEntry {
            offset: 0x30,
            symbol_name: "".to_string(),
            addend: 0x2000,
            flags: RelocationFlags::Elf {
                r_type: elf::abi::R_ARM_ABS32,
            },
        },
    ]
}
