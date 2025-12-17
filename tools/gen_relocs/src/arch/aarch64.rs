use crate::common::RelocEntry;
use object::write::RelocationFlags;

/// Generate PLT (Procedure Linkage Table) for AArch64 architecture
pub(crate) fn generate_plt(
    _base_addr: u64,
    relocs: &[RelocEntry],
    got_offset: u64,
    _next_got_index: u64,
) -> (Vec<u8>, std::collections::HashMap<String, u64>) {
    let mut text_data = vec![];
    let mut plt_entries_map = std::collections::HashMap::new();
    let _got_index = 3; // GOT slots 0-2 are reserved

    // PLT[0]: Special entry for dynamic linker
    // stp x16, x30, [sp, #-16]!
    // ldr x16, #<GOT[1]>  (link_map)
    // ldr x17, #<GOT[2]>  (_dl_runtime_resolve)
    // br x17
    text_data.extend_from_slice(&[
        0xf0, 0x5f, 0xbc, 0xa9, // stp x16, x30, [sp, #-16]!
        0x50, 0x00, 0x00, 0x58, // ldr x16, <GOT[1]> (will need relocation)
        0x51, 0x00, 0x00, 0x58, // ldr x17, <GOT[2]> (will need relocation)
        0x20, 0x02, 0x1f, 0xd6, // br x17
    ]);
    text_data.resize(16, 0x90); // Pad to 16 bytes

    // Collect unique external function symbols
    let mut external_funcs = std::collections::HashSet::new();
    for reloc in relocs {
        let r_type = match reloc.flags {
            RelocationFlags::Elf { r_type } => r_type as u64,
            _ => continue,
        };
        if r_type == elf::abi::R_AARCH64_JUMP_SLOT as u64 {
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

        // PLT entry for AArch64:
        // ldr x16, <GOT entry>  (immediate offset needs to reach GOT entry)
        // br x16                (jump to resolved address)
        // nop (padding)
        // nop (padding)

        // Calculate GOT entry offset for this function
        let _got_entry_offset = got_offset + (got_index * 8);

        // ldr uses PC-relative addressing with 19-bit signed immediate (scaled by 4 for 64-bit)
        // For now, offset placeholder; proper relocation will handle this

        // ldr x16 with offset placeholder (will need relocation)
        text_data.extend_from_slice(&[
            0x50, 0x00, 0x00, 0x58, // ldr x16, <GOT entry> (will be filled by relocation)
            0x00, 0x02, 0x1f, 0xd6, // br x16
            0x1f, 0x20, 0x03, 0xd5, // nop
            0x1f, 0x20, 0x03, 0xd5, // nop
        ]);

        got_index += 1;
    }

    if text_data.len() < 64 {
        text_data.resize(64, 0x90);
    }

    (text_data, plt_entries_map)
}

pub(crate) fn get_relocs_static() -> Vec<RelocEntry> {
    // static-like relocations (use ABS64 for symbolics and local PC-rel where useful)
    vec![
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME.to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_AARCH64_ABS64,
            },
        ),
        RelocEntry::new(
            crate::EXTERNAL_VAR_NAME.to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_AARCH64_GLOB_DAT,
            },
        ),
        RelocEntry::new(
            "".to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_AARCH64_ABS64,
            },
        ),
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME.to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_AARCH64_JUMP_SLOT,
            },
        ),
        RelocEntry::new(
            "".to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_AARCH64_ABS64,
            },
        ),
    ]
}
