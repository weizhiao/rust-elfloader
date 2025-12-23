use crate::common::{
    RelocEntry, ShdrType, SymbolDesc, SymbolScope as CommonSymbolScope, SymbolType,
};
use crate::Arch;
use anyhow::Result;
use object::{
    write::{Object, Relocation, Symbol, SymbolSection},
    Architecture, BinaryFormat, Endianness, SectionKind, SymbolKind, SymbolScope,
};
use std::collections::HashMap;

pub struct StaticElfOutput {
    pub data: Vec<u8>,
    pub reloc_offsets: Vec<u64>,
}

pub fn gen_static_elf(
    arch: Arch,
    symbols: &[SymbolDesc],
    relocs: &[RelocEntry],
) -> Result<StaticElfOutput> {
    let obj_arch: Architecture = arch.into();
    let mut obj = Object::new(BinaryFormat::Elf, obj_arch, Endianness::Little);

    let mut section_map = HashMap::new();
    let mut symbol_map = HashMap::new();
    let mut reloc_offsets = Vec::new();

    // First pass: create sections and add defined symbols
    for sym_desc in symbols {
        if let Some(content) = &sym_desc.content {
            let section_id = *section_map.entry(content.shdr_type).or_insert_with(|| {
                let (name, kind) = match content.shdr_type {
                    ShdrType::Text => (".text", SectionKind::Text),
                    ShdrType::Data => (".data", SectionKind::Data),
                    ShdrType::Plt => (".plt", SectionKind::Text),
                    ShdrType::Tls => (".tdata", SectionKind::Tls),
                    _ => (".data", SectionKind::Data),
                };
                obj.add_section(vec![], name.as_bytes().to_vec(), kind)
            });

            let offset = obj.append_section_data(section_id, &content.data, 8);

            let symbol_id = obj.add_symbol(Symbol {
                name: sym_desc.name.as_bytes().to_vec(),
                value: offset,
                size: content.data.len() as u64,
                kind: match sym_desc.sym_type {
                    SymbolType::Func => SymbolKind::Text,
                    SymbolType::Object => SymbolKind::Data,
                    SymbolType::Tls => SymbolKind::Tls,
                },
                scope: match sym_desc.scope {
                    CommonSymbolScope::Global => SymbolScope::Dynamic,
                    CommonSymbolScope::Local => SymbolScope::Compilation,
                    CommonSymbolScope::Weak => SymbolScope::Dynamic,
                },
                weak: sym_desc.scope == CommonSymbolScope::Weak,
                section: SymbolSection::Section(section_id),
                flags: object::SymbolFlags::None,
            });
            symbol_map.insert(sym_desc.name.clone(), symbol_id);
        }
    }

    // Second pass: add undefined symbols
    for sym_desc in symbols {
        if sym_desc.content.is_none() {
            let symbol_id = obj.add_symbol(Symbol {
                name: sym_desc.name.as_bytes().to_vec(),
                value: 0,
                size: 0,
                kind: match sym_desc.sym_type {
                    SymbolType::Func => SymbolKind::Text,
                    SymbolType::Object => SymbolKind::Data,
                    SymbolType::Tls => SymbolKind::Tls,
                },
                scope: SymbolScope::Dynamic,
                weak: sym_desc.scope == CommonSymbolScope::Weak,
                section: SymbolSection::Undefined,
                flags: object::SymbolFlags::None,
            });
            symbol_map.insert(sym_desc.name.clone(), symbol_id);
        }
    }

    // Add relocations
    // For relocatable objects, we usually put relocations in the section they apply to.
    // Here we assume all relocations apply to the first section that has data, or we need more info.
    // In the dylib case, relocations are more global.
    // Let's assume for now they apply to the .text section if it exists, or .data.

    let target_section_id = section_map
        .get(&ShdrType::Text)
        .or_else(|| section_map.get(&ShdrType::Data))
        .copied();

    if let Some(section_id) = target_section_id {
        let word_size = if arch.is_64() { 8 } else { 4 };
        for (idx, reloc) in relocs.iter().enumerate() {
            let symbol_id = if reloc.symbol_name.is_empty() {
                // Section-relative relocation
                obj.section_symbol(section_id)
            } else {
                *symbol_map.get(&reloc.symbol_name).ok_or_else(|| {
                    anyhow::anyhow!("Symbol not found for relocation: {}", reloc.symbol_name)
                })?
            };

            // Auto-calculate offset based on relocation sequence
            // Each relocation is word_size bytes apart starting from offset 0x10
            let offset = 0x10 + (idx as u64 * word_size);
            reloc_offsets.push(offset);

            let flags = object::write::RelocationFlags::Elf {
                r_type: reloc.r_type.0,
            };

            obj.add_relocation(
                section_id,
                Relocation {
                    offset,
                    symbol: symbol_id,
                    addend: 0, // Should we allow specifying addend in RelocEntry?
                    flags,
                },
            )?;
        }
    }

    // Write object file bytes
    let elf_data = obj.write()?;

    Ok(StaticElfOutput {
        data: elf_data,
        reloc_offsets,
    })
}
