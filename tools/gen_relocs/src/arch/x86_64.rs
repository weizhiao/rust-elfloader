use crate::common::RelocEntry;
use elf::abi::*;
use object::write::RelocationFlags;

/// Generate PLT (Procedure Linkage Table) for x86_64 architecture
///
/// # Arguments
/// * `base_addr` - Base address for memory mapping (used for RIP-relative calculations)
/// * `relocs` - List of relocations to determine PLT entries needed
/// * `got_offset` - Offset of GOT table in data segment
/// * `_next_got_index` - Next available GOT index for PLT entries
///
/// # Returns
/// (text_data, plt_entries_map)
/// - text_data: Generated PLT code
/// - plt_entries_map: Maps symbol names to their PLT entry offsets (relative to PLT start)
pub(crate) fn generate_plt(
    _base_addr: u64,
    relocs: &[RelocEntry],
    got_offset: u64,
    _next_got_index: u64,
) -> (Vec<u8>, std::collections::HashMap<String, u64>) {
    let mut text_data = vec![];
    let mut plt_entries_map = std::collections::HashMap::new();
    let mut got_index = 3; // GOT slots 0-2 are reserved (link_map, _dl_runtime_resolve, etc.)

    // PLT[0]: Special entry for dynamic linker
    // push qword [rip+offset] ; push GOT[1] (link_map)
    // jmp qword [rip+offset]  ; jump to GOT[2] (_dl_runtime_resolve)

    // Calculate RIP-relative offsets for PLT[0]
    // Text segment offset = 0x1000, Data segment offset = 0x2000
    let text_offset = 0x1000u64;
    let data_offset = 0x2000u64;

    // push instruction: offset to GOT[1]
    // RIP after push instruction = text_offset + 0 + 6
    // GOT[1] = data_offset + got_offset + 8
    let push_rip = text_offset + 6;
    let got1 = data_offset + got_offset + 8;
    let push_offset = (got1 as i64 - push_rip as i64) as i32;

    // jmp instruction: offset to GOT[2]
    // RIP after jmp instruction = text_offset + 0 + 12
    // GOT[2] = data_offset + got_offset + 16
    let jmp_rip = text_offset + 12;
    let got2 = data_offset + got_offset + 16;
    let jmp_offset = (got2 as i64 - jmp_rip as i64) as i32;

    text_data.extend_from_slice(&[0xff, 0x35]); // push qword [rip+offset]
    text_data.extend_from_slice(&push_offset.to_le_bytes());
    text_data.extend_from_slice(&[0xff, 0x25]); // jmp qword [rip+offset]
    text_data.extend_from_slice(&jmp_offset.to_le_bytes());
    text_data.resize(16, 0x90); // Pad to 16 bytes with NOPs

    // Collect unique external function symbols that need PLT entries
    let mut external_funcs = std::collections::HashSet::new();
    for reloc in relocs {
        let r_type = match reloc.flags {
            RelocationFlags::Elf { r_type } => r_type as u64,
            _ => continue,
        };
        // Check if this is a PLT-related relocation
        if r_type == R_X86_64_PLT32 as u64 || r_type == R_X86_64_JUMP_SLOT as u64 {
            if !reloc.symbol_name.is_empty() {
                external_funcs.insert(reloc.symbol_name.clone());
            }
        }
    }

    // Generate PLT entries for each external function
    let mut reloc_index = 0u32;
    for func_name in external_funcs {
        let plt_entry_offset = text_data.len() as u64;
        plt_entries_map.insert(func_name, plt_entry_offset);

        // Calculate GOT entry offset for this function
        // GOT entries start at got_offset in data segment
        // GOT[0-2] are reserved, GOT[3+n] are for PLT functions
        let got_entry_offset = got_offset + (got_index * 8);

        // The jmp instruction is PC-relative (RIP-relative addressing)
        // jmp is 6 bytes (0xff 0x25 + 4 byte offset)
        // After jmp executes, RIP points to next instruction at plt_entry_offset + 6
        // We need offset relative to that point
        //
        // Text segment starts at base_addr + text_offset
        // Data segment starts at base_addr + data_offset
        // In the current layout: text is at page 1, data is at page 2
        // text_vaddr = base_addr + 0x1000, data_vaddr = base_addr + 0x2000
        // GOT is at data_vaddr + got_offset = base_addr + 0x2000 + got_offset
        // PLT entry is at text_vaddr + plt_entry_offset = base_addr + 0x1000 + plt_entry_offset
        // After jmp, RIP = base_addr + 0x1000 + plt_entry_offset + 6
        // Target = base_addr + 0x2000 + got_entry_offset
        // Offset = Target - RIP = (base_addr + 0x2000 + got_entry_offset) - (base_addr + 0x1000 + plt_entry_offset + 6)
        //        = 0x1000 + got_entry_offset - plt_entry_offset - 6
        let text_offset = 0x1000u64; // Text segment offset from base_addr
        let data_offset = 0x2000u64; // Data segment offset from base_addr
        let rip_after_jmp = text_offset + plt_entry_offset + 6;
        let got_target = data_offset + got_entry_offset;
        let got_offset_rel = (got_target as i64 - rip_after_jmp as i64) as i32;

        // Calculate offset for final jmp back to PLT[0]
        // This PLT entry ends at plt_entry_offset + 16 bytes
        // PLT[0] is at offset 0
        // jmp uses signed 32-bit relative offset: target = rip + offset
        // After jmp, RIP = plt_entry_offset + 16 (next instruction after jmp)
        // We want to jump to offset 0, so: 0 = (plt_entry_offset + 16) + offset
        // Therefore: offset = -(plt_entry_offset + 16)
        let plt0_offset = -((plt_entry_offset as i32) + 16);

        // PLT entry:
        // jmp qword [rip+offset] ; jump to GOT entry (needs proper offset or relocation)
        // push index            ; push relocation index for dynamic linker
        // jmp PLT[0]           ; jump back to resolver stub
        text_data.extend_from_slice(&[
            0xff, 0x25, // jmp qword [rip+offset]
        ]);
        text_data.extend_from_slice(&got_offset_rel.to_le_bytes());

        text_data.extend_from_slice(&[
            0x68, // push immediate (32-bit)
        ]);
        text_data.extend_from_slice(&reloc_index.to_le_bytes());

        text_data.extend_from_slice(&[
            0xe9, // jmp rel32 (PC-relative)
        ]);
        text_data.extend_from_slice(&plt0_offset.to_le_bytes());

        reloc_index += 1;
        got_index += 1;
    }

    // Ensure minimum size
    if text_data.len() < 64 {
        text_data.resize(64, 0x90);
    }

    (text_data, plt_entries_map)
}

pub(crate) fn get_relocs_static() -> Vec<RelocEntry> {
    vec![
        // Absolute
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME,
            RelocationFlags::Elf {
                r_type: R_X86_64_64,
            },
        ),
        // PC-relative / GOTPCREL
        RelocEntry::new(
            crate::EXTERNAL_VAR_NAME,
            RelocationFlags::Elf {
                r_type: R_X86_64_GOTPCREL,
            },
        ),
        // Use GOTPCREL for PIC-friendly PC-relative access
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME,
            RelocationFlags::Elf {
                r_type: R_X86_64_GOTPCREL,
            },
        ),
        // PLT/PC32 to cause PLT entry generation by linker
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME,
            RelocationFlags::Elf {
                r_type: R_X86_64_PLT32,
            },
        ),
        // Relative-like relocation to data section
        RelocEntry::new(
            "",
            RelocationFlags::Elf {
                r_type: R_X86_64_64,
            },
        ),
        // TLS-like relocation (using 64-bit absolute for now)
        RelocEntry::new(
            crate::EXTERNAL_VAR_NAME,
            RelocationFlags::Elf {
                r_type: R_X86_64_64,
            },
        ),
    ]
}
