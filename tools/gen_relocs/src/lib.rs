use anyhow::Result;
use clap::ValueEnum;
use object::write::{Object, Relocation, SectionKind, Symbol, SymbolSection};
use object::{Architecture, BinaryFormat, Endianness, SymbolKind, SymbolScope};
use std::fs::File;
use std::io::Write;
use std::path::Path;

mod arch;
mod common;
pub mod writer;

pub use common::RelocEntry;
use common::RelocFixtures;

// String forms for easy use by other crates/tests.
pub const EXTERNAL_FUNC_NAME: &str = "external_func";
pub const EXTERNAL_VAR_NAME: &str = "external_var";
pub const LOCAL_VAR_NAME: &str = "local_var";
pub const EXTERNAL_TLS_NAME: &str = "external_tls";

// Exported relocation offsets so tests can reference generator layout.
pub const RELOC_OFF_ABS: usize = 0x10;
pub const RELOC_OFF_GOT: usize = 0x18;
pub const RELOC_OFF_PC_REL: usize = 0x20;
pub const RELOC_OFF_PLT: usize = 0x28;
pub const RELOC_OFF_RELATIVE: usize = 0x30;
pub const RELOC_OFF_DTPOFF: usize = 0x40;
pub const RELOC_OFF_IRELATIVE: usize = 0x48;
pub const RELOC_OFF_COPY: usize = 0x50;
pub const RELOC_OFF_TLS: usize = 0x38;

// Offset of `local_var` inside .data
pub const LOCAL_VAR_OFF: usize = 0x20;
// Default example addresses used in dynamic fixtures (resolver / external var)
pub const EXAMPLE_FUNC_ADDR: usize = 0x1000;
pub const EXAMPLE_VAR_ADDR: usize = 0x2000;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Riscv64,
    Riscv32,
    Arm,
}

impl From<Arch> for Architecture {
    fn from(arch: Arch) -> Self {
        match arch {
            Arch::X86_64 => Architecture::X86_64,
            Arch::Aarch64 => Architecture::Aarch64,
            Arch::Riscv64 => Architecture::Riscv64,
            Arch::Riscv32 => Architecture::Riscv32,
            Arch::Arm => Architecture::Arm,
        }
    }
}

pub fn gen_static_elf(out_path: &Path, arch: Arch) -> Result<()> {
    let obj_arch: Architecture = arch.into();
    let mut obj = Object::new(BinaryFormat::Elf, obj_arch, Endianness::Little);

    // Add .data and .text sections
    let data_id = obj.add_section(vec![], b".data".to_vec(), SectionKind::Data);
    let text_id = obj.add_section(vec![], b".text".to_vec(), SectionKind::Text);
    // Add TLS data section
    let tdata_id = obj.add_section(vec![], b".tdata".to_vec(), SectionKind::Tls);

    // Put some placeholder bytes into sections
    let data_bytes = vec![0u8; 0x100];
    obj.append_section_data(data_id, &data_bytes, 8);
    let text_bytes = vec![0x90u8; 16];
    obj.append_section_data(text_id, &text_bytes, 1);

    // For testing expose external symbols so the loader can resolve them.
    let ext_scope = SymbolScope::Dynamic;

    // Add symbols: undefined external_func and external_var, and local defined symbol local_var
    let external_func = obj.add_symbol(Symbol {
        name: EXTERNAL_FUNC_NAME.as_bytes().to_vec(),
        value: 0,
        size: 0,
        kind: SymbolKind::Text,
        scope: ext_scope,
        weak: false,
        section: SymbolSection::Undefined,
        flags: object::SymbolFlags::None,
    });

    let external_var = obj.add_symbol(Symbol {
        name: EXTERNAL_VAR_NAME.as_bytes().to_vec(),
        value: 0,
        size: 8,
        kind: SymbolKind::Data,
        scope: ext_scope,
        weak: false,
        section: SymbolSection::Undefined,
        flags: object::SymbolFlags::None,
    });

    // TLS symbol: declare as undefined so linker will treat it as external
    let external_tls = obj.add_symbol(Symbol {
        name: EXTERNAL_TLS_NAME.as_bytes().to_vec(),
        value: 0,
        size: 8,
        kind: SymbolKind::Data, // try SymbolKind::Tls if available
        scope: ext_scope,
        weak: false,
        section: SymbolSection::Undefined,
        flags: object::SymbolFlags::None,
    });

    // local_var defined in data at offset 0x20
    obj.add_symbol(Symbol {
        name: LOCAL_VAR_NAME.as_bytes().to_vec(),
        value: 0x20,
        size: 4,
        kind: SymbolKind::Data,
        scope: SymbolScope::Dynamic,
        weak: false,
        section: SymbolSection::Section(data_id),
        flags: object::SymbolFlags::None,
    });

    let data_section_sym = obj.section_symbol(data_id);
    let _tdata_section_sym = obj.section_symbol(tdata_id);

    // Create symbol name to SymbolId mapping
    let mut symbol_map = std::collections::HashMap::new();
    symbol_map.insert(EXTERNAL_FUNC_NAME.to_string(), external_func);
    symbol_map.insert(EXTERNAL_VAR_NAME.to_string(), external_var);
    symbol_map.insert(EXTERNAL_TLS_NAME.to_string(), external_tls);
    symbol_map.insert("".to_string(), data_section_sym);  // Empty string for section-relative relocations

    let fixtures = RelocFixtures {
        data_id,
        external_func,
        external_var,
        external_tls,
        data_section_sym,
    };

    let relocs = match obj_arch {
        Architecture::X86_64 => arch::x86_64::get_relocs_static(),
        Architecture::Aarch64 => arch::aarch64::get_relocs_static(),
        Architecture::Riscv64 => arch::riscv64::get_relocs_static(),
        Architecture::Riscv32 => arch::riscv32::get_relocs_static(),
        Architecture::Arm => arch::arm::get_relocs_static(),
        _ => Vec::new(),
    };

    for reloc in &relocs {
        let symbol_id = symbol_map.get(&reloc.symbol_name).copied().unwrap_or(data_section_sym);
        obj.add_relocation(
            fixtures.data_id,
            Relocation {
                offset: reloc.offset,
                symbol: symbol_id,
                addend: reloc.addend,
                flags: reloc.flags,
            },
        )?;
    }

    // Write object file bytes
    let elf_data = obj.write()?;

    let mut f = File::create(out_path)?;
    f.write_all(&elf_data)?;
    println!("Wrote {}", out_path.display());
    Ok(())
}

fn gen_dynamic_elf(out_path: &Path, arch: Arch) -> Result<()> {
    let relocs = arch::get_relocs_dynamic(arch);
    let mut out = out_path.to_path_buf();
    if out.extension().and_then(|s| s.to_str()) != Some("so") {
        out.set_extension("so");
    }

    // Use default configuration for standard ELF generation
    let writer = writer::ElfWriter::new(arch);
    let symbols = vec![
        writer::SymbolDesc::local_var(LOCAL_VAR_NAME, crate::LOCAL_VAR_OFF as u16),
        writer::SymbolDesc::global_func(EXTERNAL_FUNC_NAME),
        writer::SymbolDesc::global_object(EXTERNAL_VAR_NAME),
        writer::SymbolDesc::global_object(EXTERNAL_TLS_NAME),
    ];
    let _ = writer.write(&out, &relocs, &symbols)?;
    println!("Wrote {}", out.display());
    Ok(())
}

/// Generate dynamic ELF and return the output metadata for verification
pub fn gen_dynamic_elf_with_output(arch: Arch) -> Result<writer::ElfWriteOutput> {
    let relocs = arch::get_relocs_dynamic(arch);
    let writer = writer::ElfWriter::new(arch);
    let symbols = vec![
        writer::SymbolDesc::local_var(LOCAL_VAR_NAME, crate::LOCAL_VAR_OFF as u16),
        writer::SymbolDesc::global_func(EXTERNAL_FUNC_NAME),
        writer::SymbolDesc::global_object(EXTERNAL_VAR_NAME),
        writer::SymbolDesc::global_object(EXTERNAL_TLS_NAME),
    ];
    writer.write_elf(&relocs, &symbols)
}

pub fn get_relocs_dynamic(arch: Arch) -> Vec<common::RelocEntry> {
    arch::get_relocs_dynamic(arch)
}

pub fn gen_elf(out_path: &Path, arch: Arch, dynamic: bool) -> Result<()> {
    if dynamic {
        gen_dynamic_elf(out_path, arch)?;
    } else {
        gen_static_elf(out_path, arch)?;
    }
    Ok(())
}
