use crate::common::RelocEntry;
use object::write::RelocationFlags;

/// Generate PLT (Procedure Linkage Table) for ARM architecture
pub(crate) fn generate_plt(
    _base_addr: u64,
    relocs: &[RelocEntry],
    got_offset: u64,
    _next_got_index: u64,
) -> (Vec<u8>, std::collections::HashMap<String, u64>) {
    let mut text_data = vec![];
    let mut plt_entries_map = std::collections::HashMap::new();

    // PLT[0]: Special entry for dynamic linker
    // push {r4, ip}
    // ldr ip, [pc]  ; load GOT[1] (link_map)
    // ldr pc, [pc]  ; load and jump to GOT[2] (_dl_runtime_resolve)
    text_data.extend_from_slice(&[
        0x04, 0xc0, 0x2d, 0xe5, // push {r4, ip}
        0x04, 0xc0, 0x9f, 0xe5, // ldr ip, [pc, #4] (will need relocation)
        0x04, 0xf0, 0x9f, 0xe5, // ldr pc, [pc, #4] (will need relocation)
    ]);
    text_data.resize(16, 0x00); // Pad to 16 bytes

    // Collect unique external function symbols
    let mut external_funcs = std::collections::HashSet::new();
    for reloc in relocs {
        let r_type = match reloc.flags {
            RelocationFlags::Elf { r_type } => r_type as u64,
            _ => continue,
        };
        if r_type == elf::abi::R_ARM_JUMP_SLOT as u64 {
            if !reloc.symbol_name.is_empty() {
                external_funcs.insert(reloc.symbol_name.clone());
            }
        }
    }

    // Generate PLT entries
    let mut got_index = 3; // GOT[0-2] are reserved
    for func_name in external_funcs {
        let plt_entry_offset = text_data.len() as u64;
        plt_entries_map.insert(func_name, plt_entry_offset);

        // PLT entry for ARM:
        // ldr pc, [pc, #-4]  ; load and jump to GOT entry
        // GOT entry address (will be relocated)

        // Calculate GOT entry offset for this function
        let _got_entry_offset = got_offset + (got_index * 4);

        text_data.extend_from_slice(&[
            0x04, 0xf0, 0x9f, 0xe5, // ldr pc, [pc, #-4]
        ]);
        // GOT entry address placeholder (needs relocation)
        text_data.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x00, // GOT entry address (will be filled by relocation)
        ]);

        got_index += 1;
    }

    if text_data.len() < 64 {
        text_data.resize(64, 0x00);
    }

    (text_data, plt_entries_map)
}

pub fn get_relocs_static() -> Vec<RelocEntry> {
    vec![
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME.to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_ARM_ABS32,
            },
        ),
        RelocEntry::new(
            crate::EXTERNAL_VAR_NAME.to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_ARM_GLOB_DAT,
            },
        ),
        RelocEntry::new(
            "".to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_ARM_ABS32,
            },
        ),
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME.to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_ARM_JUMP_SLOT,
            },
        ),
        RelocEntry::new(
            "".to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_ARM_ABS32,
            },
        ),
    ]
}
