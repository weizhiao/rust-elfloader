use crate::common::RelocEntry;
use object::write::RelocationFlags;

/// Generate PLT (Procedure Linkage Table) for RISC-V 64 architecture
pub(crate) fn generate_plt(
    _base_addr: u64,
    relocs: &[RelocEntry],
    got_offset: u64,
    _next_got_index: u64,
) -> (Vec<u8>, std::collections::HashMap<String, u64>) {
    let mut text_data = vec![];
    let mut plt_entries_map = std::collections::HashMap::new();

    // PLT[0]: Special entry for dynamic linker
    // addi sp, sp, -16
    // sd ra, 8(sp)
    // ld t0, offset(GOT[1])   ; load link_map
    // ld t1, offset+8(GOT[2]) ; load _dl_runtime_resolve
    // jr t1
    text_data.extend_from_slice(&[
        0x13, 0x01, 0x01, 0xff, // addi sp, sp, -16
        0x23, 0x34, 0x11, 0x00, // sd ra, 8(sp)
        0x83, 0x22, 0x00, 0x00, // ld t0, 0(GOT[1]) (will need relocation)
        0x03, 0x23, 0x80, 0x00, // ld t1, 8(GOT[2]) (will need relocation)
        0x67, 0x00, 0x03, 0x00, // jr t1
    ]);
    text_data.resize(16, 0x13); // Pad with ADDI (nop)

    // Collect unique external function symbols
    let mut external_funcs = std::collections::HashSet::new();
    for reloc in relocs {
        let r_type = match reloc.flags {
            RelocationFlags::Elf { r_type } => r_type as u64,
            _ => continue,
        };
        if r_type == elf::abi::R_RISCV_JUMP_SLOT as u64 {
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

        // PLT entry for RISC-V 64-bit:
        // ld t0, offset(GOT)   - load GOT entry address
        // jr t0                - jump to resolved address
        // nop (padding)
        // nop (padding)

        // Calculate GOT entry offset for this function
        let got_entry_offset = got_offset + (got_index * 8);

        // ld instruction uses 12-bit signed immediate
        let got_offset_imm = got_entry_offset as i32 & 0xfff;

        text_data.extend_from_slice(&[
            0x83, 0x22, // ld t0, offset(x0)
        ]);
        // Offset placeholder - will be set by relocation
        text_data.extend_from_slice(&(got_offset_imm as i16).to_le_bytes());

        text_data.extend_from_slice(&[
            0x67, 0x00, 0x05, 0x00, // jr t0
            0x13, 0x00, 0x00, 0x00, // nop (addi x0, x0, 0)
            0x13, 0x00, 0x00, 0x00, // nop
        ]);

        got_index += 1;
    }

    if text_data.len() < 64 {
        text_data.resize(64, 0x13);
    }

    (text_data, plt_entries_map)
}

pub fn get_relocs_static() -> Vec<RelocEntry> {
    vec![
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME.to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_RISCV_64,
            },
        ),
        RelocEntry::new(
            crate::EXTERNAL_VAR_NAME.to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_RISCV_64,
            },
        ),
        RelocEntry::new(
            "".to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_RISCV_64,
            },
        ),
        RelocEntry::new(
            crate::EXTERNAL_FUNC_NAME.to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_RISCV_JUMP_SLOT,
            },
        ),
        RelocEntry::new(
            "".to_string(),
            RelocationFlags::Elf {
                r_type: elf::abi::R_RISCV_64,
            },
        ),
    ]
}
