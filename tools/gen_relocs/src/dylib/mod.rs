use crate::common::{RelocEntry, ShdrType, SymbolDesc};
use crate::dylib::data::DataMetaData;
use crate::dylib::dynamic::DynamicMetadata;
use crate::dylib::layout::ElfLayout;
use crate::dylib::reloc::RelocMetaData;
use crate::dylib::shdr::{SectionAllocator, ShdrManager};
use crate::dylib::symtab::SymTabMetadata;
use crate::dylib::text::CodeMetaData;
use crate::Arch;
use anyhow::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use elf::abi::*;
use std::path::Path;

mod data;
mod dynamic;
mod layout;
pub(crate) mod reloc;
pub(crate) mod shdr;
pub(crate) mod symtab;
pub(crate) mod text;

fn align_up(val: u64, align: u64) -> u64 {
    (val + align - 1) / align * align
}

pub(crate) struct StringTable {
    data: Vec<u8>,
}

impl StringTable {
    pub(crate) fn new() -> Self {
        Self { data: vec![0u8] } // initial null byte
    }

    pub(crate) fn add(&mut self, s: &str) -> u32 {
        let idx = self.data.len() as u32;
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0);
        idx
    }

    pub(crate) fn cur_idx(&self) -> u32 {
        self.data.len() as u32
    }

    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }

    pub(crate) fn get_str(&self, start: usize) -> &str {
        unsafe {
            let ptr = self.data.as_ptr().add(start);
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap()
        }
    }
}
/// Configuration for ELF writer with customizable parameters.
///
/// This struct allows fine-tuning of ELF generation parameters:
/// - `base_addr`: Virtual address where the ELF will be loaded
/// - `page_size`: Alignment for program headers (typically 0x1000)
/// - `initial_data_size`: Size of the .data segment
/// - `section_align`: Alignment for ELF sections
///
/// # Example
///
/// ```ignore
/// let config = ElfWriterConfig::default()
///     .with_base_addr(0x7f000000)
///     .with_page_size(0x2000);
/// let writer = ElfWriter::with_config(Arch::X86_64, config);
/// ```
#[derive(Clone)]
pub struct ElfWriterConfig {
    /// Base address for memory mapping (default: 0x400000)
    pub base_addr: u64,
    /// Page size for alignment (default: 0x1000)
    pub page_size: u64,
}

impl ElfWriterConfig {
    /// Create a default configuration
    pub fn default() -> Self {
        Self {
            base_addr: 0x400000,
            page_size: 0x1000,
        }
    }

    /// Set custom base address
    pub fn with_base_addr(mut self, addr: u64) -> Self {
        self.base_addr = addr;
        self
    }

    /// Set custom page size
    pub fn with_page_size(mut self, size: u64) -> Self {
        self.page_size = size;
        self
    }
}

/// Relocation metadata for testing and verification
#[derive(Clone, Debug)]
pub struct RelocationInfo {
    /// Virtual address of the relocation in mapped memory
    pub vaddr: u64,
    /// Relocation type
    pub r_type: u32,
    /// Symbol index
    pub sym_idx: u64,
    /// Addend value for relocation calculation
    pub addend: i64,
}

/// Output of ELF generation containing the file data and relocation metadata
#[derive(Clone)]
pub struct ElfWriteOutput {
    /// Raw ELF file bytes
    pub data: Vec<u8>,
    /// Base address used during ELF generation
    pub base_addr: u64,
    /// Virtual address of the data segment
    pub data_vaddr: u64,
    /// Virtual address of the text segment
    pub text_vaddr: u64,
    /// Relocation entries for verification
    pub relocations: Vec<RelocationInfo>,
}

// Constants to replace magic numbers
const EHDR_SIZE_64: u64 = 64;
const EHDR_SIZE_32: u64 = 52;
const PHDR_SIZE_64: u64 = 56;
const PHDR_SIZE_32: u64 = 32;
const SHDR_SIZE_64: u64 = 64;
const SHDR_SIZE_32: u64 = 40;
const SYM_SIZE_64: u64 = 24;
const SYM_SIZE_32: u64 = 16;
const RELA_SIZE_64: u64 = 24;
const RELA_SIZE_32: u64 = 12;
const REL_SIZE_64: u64 = 16;
const REL_SIZE_32: u64 = 8;
const DYN_SIZE_64: u64 = 16;
const DYN_SIZE_32: u64 = 8;
const HASH_SIZE: u64 = 4;

pub struct DylibWriter {
    arch: Arch,
    is_64: bool,
    is_rela: bool,
    config: ElfWriterConfig,
}

impl DylibWriter {
    /// Create a new ELF writer with default configuration
    pub fn new(arch: Arch) -> Self {
        let config = ElfWriterConfig::default();
        Self {
            arch,
            is_64: arch.is_64(),
            is_rela: arch.is_rela(),
            config,
        }
    }

    /// Create a new ELF writer with custom configuration
    pub fn with_config(arch: Arch, config: ElfWriterConfig) -> Self {
        Self {
            arch,
            is_64: arch.is_64(),
            is_rela: arch.is_rela(),
            config,
        }
    }

    /// Write ELF to file, returning the output metadata for verification
    pub fn write_file(
        &self,
        out_path: &Path,
        relocs: &[RelocEntry],
        symbols: &[SymbolDesc],
    ) -> Result<ElfWriteOutput> {
        let output = self.write(relocs, symbols)?;
        std::fs::write(out_path, &output.data)?;
        Ok(output)
    }

    pub fn write(
        &self,
        raw_relocs: &[RelocEntry],
        symbols: &[SymbolDesc],
    ) -> Result<ElfWriteOutput> {
        let mut allocator = SectionAllocator::new();
        let mut symtab = SymTabMetadata::new(self.arch);
        symtab.add_symbols(symbols);
        let mut reloc = RelocMetaData::new(self.arch, raw_relocs, &symtab, &mut allocator)?;
        symtab.add_plt_symbols(&reloc);

        let data = DataMetaData::new(&reloc, &symtab, self.is_64, &mut allocator);
        let mut text = CodeMetaData::new(&symtab, &mut allocator);

        // 1. Create initial sections
        let mut sections = vec![];
        text.create_sections(&allocator, &mut sections);
        data.create_sections(&allocator, &mut sections);
        symtab.create_sections(self.is_64, &mut allocator, &mut sections);
        reloc.create_sections(self.is_rela, self.is_64, &mut allocator, &mut sections)?;

        // 2. Create .dynamic section (placeholder)
        let mut dyn_meta = DynamicMetadata::new();
        dyn_meta.create_section(&mut sections, self.is_64, self.is_rela, &mut allocator);

        // 3. Initialize ShdrManager and Layout
        let mut shdr_manager = ShdrManager::new();
        for sec in sections {
            shdr_manager.add_section(sec.header, sec.data);
        }

        // 4. Build .shstrtab
        shdr_manager.create_shstrtab_section(&mut allocator);

        // 5. Layout
        let mut layout = ElfLayout::new(&self.config);
        let ehdr_size = if self.is_64 {
            EHDR_SIZE_64
        } else {
            EHDR_SIZE_32
        };
        let ph_size = (if self.is_64 {
            PHDR_SIZE_64
        } else {
            PHDR_SIZE_32
        }) * shdr_manager.get_phnum() as u64;
        layout.add_header(ehdr_size, ph_size);
        shdr_manager.layout(&mut layout);

        // 6. Update all metadata with final addresses
        let shdr_map = shdr_manager.get_shdr_map();
        let plt_vaddr = shdr_manager.get_vaddr(ShdrType::Plt);
        let text_vaddr = shdr_manager.get_vaddr(ShdrType::Text);
        let data_vaddr = shdr_manager.get_vaddr(ShdrType::Data);
        let got_vaddr = shdr_manager.get_vaddr(ShdrType::Got);

        // Update symbols
        symtab.update_symbol_values(plt_vaddr, text_vaddr, data_vaddr, &shdr_map);
        symtab.refill_dynsym(&mut allocator, self.is_64);

        // Update PLT and Text
        text.refill_text(plt_vaddr, text_vaddr, got_vaddr, &symtab, &mut allocator);

        // Update Relocations
        let got_id = shdr_manager.get_data_id(ShdrType::Got);
        reloc.fill_reloc(
            self.is_rela,
            self.is_64,
            &got_id,
            got_vaddr,
            data_vaddr,
            &symtab,
            &mut allocator,
        )?;

        // Update Dynamic
        dyn_meta.update_entry(DT_STRTAB as i64, shdr_manager.get_vaddr(ShdrType::DynStr));
        dyn_meta.update_entry(DT_SYMTAB as i64, shdr_manager.get_vaddr(ShdrType::DynSym));
        dyn_meta.update_entry(DT_HASH as i64, shdr_manager.get_vaddr(ShdrType::Hash));
        dyn_meta.update_entry(DT_PLTGOT as i64, got_vaddr);
        if self.is_rela {
            dyn_meta.update_entry(DT_RELA as i64, shdr_manager.get_vaddr(ShdrType::RelaDyn));
            dyn_meta.update_entry(DT_JMPREL as i64, shdr_manager.get_vaddr(ShdrType::RelaPlt));
            dyn_meta.update_entry(DT_RELACOUNT as i64, reloc.relative_count() as u64);
        } else {
            dyn_meta.update_entry(DT_REL as i64, shdr_manager.get_vaddr(ShdrType::RelDyn));
            dyn_meta.update_entry(DT_JMPREL as i64, shdr_manager.get_vaddr(ShdrType::RelPlt));
            dyn_meta.update_entry(DT_RELCOUNT as i64, reloc.relative_count() as u64);
        }
        let dyn_id = shdr_manager.get_data_id(ShdrType::Dynamic);
        dyn_meta.write_to_vec(allocator.get_mut(&dyn_id), self.is_64)?;

        // 7. Write final ELF
        let mut out_bytes = vec![];
        // Write EHDR
        let ehdr = ElfHeader {
            ident: self.get_ident(),
            type_: ET_DYN,
            machine: self.get_machine(),
            version: EV_CURRENT as u32,
            entry: 0,
            phoff: ehdr_size,
            shoff: layout.file_off,
            flags: 0,
            ehsize: ehdr_size as u16,
            phentsize: (if self.is_64 {
                PHDR_SIZE_64
            } else {
                PHDR_SIZE_32
            }) as u16,
            phnum: shdr_manager.get_phnum(),
            shentsize: (if self.is_64 {
                SHDR_SIZE_64
            } else {
                SHDR_SIZE_32
            }) as u16,
            shnum: (shdr_manager.get_shdr_count() + 1) as u16,
            shstrndx: *shdr_map.get(&ShdrType::ShStrTab).unwrap() as u16,
        };
        self.write_struct(&mut out_bytes, ehdr)?;

        // Write PHDRs
        shdr_manager.write_phdrs(&mut out_bytes, self.is_64, self.config.page_size)?;

        // Write Sections
        for sec in shdr_manager.get_sections() {
            self.write_at(&mut out_bytes, sec.header.offset, allocator.get(&sec.data));
        }

        // Write SHDRs
        let mut shdr_buf = vec![];
        shdr_manager.write_shdrs(&mut shdr_buf, self.is_64)?;
        self.write_at(&mut out_bytes, layout.file_off, &shdr_buf);

        Ok(ElfWriteOutput {
            data: out_bytes,
            base_addr: self.config.base_addr,
            data_vaddr,
            text_vaddr,
            relocations: reloc.get_relocation_infos(got_vaddr),
        })
    }

    fn get_ident(&self) -> [u8; 16] {
        let mut ident = [0u8; 16];
        ident[0] = 0x7f;
        ident[1] = b'E';
        ident[2] = b'L';
        ident[3] = b'F';
        ident[4] = if self.is_64 { 2 } else { 1 };
        ident[5] = 1; // Little endian
        ident[6] = 1; // Version
        ident[7] = 0; // OS ABI
        ident
    }

    fn write_at(&self, buf: &mut Vec<u8>, offset: u64, data: &[u8]) {
        let offset = offset as usize;
        if buf.len() < offset + data.len() {
            buf.resize(offset + data.len(), 0);
        }
        buf[offset..offset + data.len()].copy_from_slice(data);
    }

    fn write_struct<T: WriteToBuf>(&self, buf: &mut Vec<u8>, item: T) -> Result<()> {
        item.write(buf, self.is_64)
    }

    fn get_machine(&self) -> u16 {
        match self.arch {
            Arch::X86_64 => EM_X86_64,
            Arch::Aarch64 => EM_AARCH64,
            Arch::Riscv64 => EM_RISCV,
            Arch::Riscv32 => EM_RISCV,
            Arch::Arm => EM_ARM,
        }
    }
}

struct ElfHeader {
    ident: [u8; 16],
    type_: u16,
    machine: u16,
    version: u32,
    entry: u64,
    phoff: u64,
    shoff: u64,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

trait WriteToBuf {
    fn write(&self, buf: &mut Vec<u8>, is_64: bool) -> Result<()>;
}

impl WriteToBuf for ElfHeader {
    fn write(&self, buf: &mut Vec<u8>, is_64: bool) -> Result<()> {
        buf.extend_from_slice(&self.ident);
        buf.write_u16::<LittleEndian>(self.type_)?;
        buf.write_u16::<LittleEndian>(self.machine)?;
        buf.write_u32::<LittleEndian>(self.version)?;
        if is_64 {
            buf.write_u64::<LittleEndian>(self.entry)?;
            buf.write_u64::<LittleEndian>(self.phoff)?;
            buf.write_u64::<LittleEndian>(self.shoff)?;
        } else {
            buf.write_u32::<LittleEndian>(self.entry as u32)?;
            buf.write_u32::<LittleEndian>(self.phoff as u32)?;
            buf.write_u32::<LittleEndian>(self.shoff as u32)?;
        }
        buf.write_u32::<LittleEndian>(self.flags)?;
        buf.write_u16::<LittleEndian>(self.ehsize)?;
        buf.write_u16::<LittleEndian>(self.phentsize)?;
        buf.write_u16::<LittleEndian>(self.phnum)?;
        buf.write_u16::<LittleEndian>(self.shentsize)?;
        buf.write_u16::<LittleEndian>(self.shnum)?;
        buf.write_u16::<LittleEndian>(self.shstrndx)?;
        Ok(())
    }
}
