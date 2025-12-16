//! ELF writer module for generating dynamic ELF libraries with relocation information.
//!
//! This module provides a flexible ELF writer that can generate shared libraries for different
//! architectures (x86_64, ARM, RISC-V, etc.) with full support for dynamic linking and relocations.
//!
//! # Features
//!
//! - Configurable base address, page size, and data segment size
//! - Support for multiple architectures through the Arch enum
//! - Generation of dynamic symbol table, relocation tables, and dynamic section
//! - Relocation metadata collection for testing and verification
//! - Support for both RELA and REL relocation formats
//!
//! # Example
//!
//! ```ignore
//! use gen_relocs::writer::{ElfWriter, ElfWriterConfig, SymbolDesc};
//! use gen_relocs::Arch;
//!
//! // Use default configuration
//! let writer = ElfWriter::new(Arch::X86_64);
//!
//! // Or with custom configuration
//! let config = ElfWriterConfig::default()
//!     .with_base_addr(0x400000)
//!     .with_page_size(0x1000);
//! let writer = ElfWriter::with_config(Arch::X86_64, config);
//! ```

use anyhow::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use elf::abi::*;
use std::collections::HashMap;
use std::path::Path;

use crate::common::RelocEntry;
use crate::{Arch, EXTERNAL_VAR_NAME};

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
    /// Initial data segment size (default: 0x3000)
    pub initial_data_size: u64,
    /// Section alignment in bytes (default: 8 for 64-bit, 4 for 32-bit)
    pub section_align: u64,
}

impl ElfWriterConfig {
    /// Create a default configuration
    pub fn default() -> Self {
        Self {
            base_addr: 0x400000,
            page_size: 0x1000,
            initial_data_size: 0x3000,
            section_align: 8,
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

    /// Set custom initial data size
    pub fn with_initial_data_size(mut self, size: u64) -> Self {
        self.initial_data_size = size;
        self
    }

    /// Set custom section alignment
    pub fn with_section_align(mut self, align: u64) -> Self {
        self.section_align = align;
        self
    }
}

/// Relocation metadata for testing and verification
#[derive(Clone, Debug)]
pub struct RelocationInfo {
    /// Virtual address of the relocation in mapped memory
    pub vaddr: u64,
    /// Relocation type
    pub r_type: u64,
    /// Symbol index
    pub sym_idx: u64,
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
const ELF_IDENT_SIZE: usize = 16;
const ELF_IDENT_MAG0: u8 = 0x7f;
const ELF_IDENT_MAG1: u8 = b'E';
const ELF_IDENT_MAG2: u8 = b'L';
const ELF_IDENT_MAG3: u8 = b'F';
const ELF_IDENT_CLASS_OFFSET: usize = 4;
const ELF_IDENT_DATA_OFFSET: usize = 5;
const ELF_IDENT_VERSION_OFFSET: usize = 6;
const ELF_IDENT_OSABI_OFFSET: usize = 7;
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
const NUM_PROGRAM_HEADERS: u64 = 3;

struct ElfLayout {
    base_addr: u64,
    page_size: u64,
    section_align: u64,
    file_off: u64,
}

impl ElfLayout {
    fn new(base_addr: u64, page_size: u64, section_align: u64) -> Self {
        Self {
            base_addr,
            page_size,
            section_align,
            file_off: 0,
        }
    }

    fn add_header(&mut self, ehdr_size: u64, ph_size: u64) {
        self.file_off = ehdr_size + ph_size;
        self.file_off = self.align_up(self.file_off, self.page_size);
    }

    fn add_segment(&mut self, size: u64) -> (u64, u64) {
        let offset = self.file_off;
        let vaddr = self.base_addr + offset;
        self.file_off = self.align_up(self.file_off + size, self.page_size);
        (offset, vaddr)
    }

    fn add_section(&mut self, size: u64) -> (u64, u64) {
        let offset = self.file_off;
        let vaddr = self.base_addr + offset;
        self.file_off = self.align_up(self.file_off + size, self.section_align);
        (offset, vaddr)
    }

    fn align_up(&self, val: u64, align: u64) -> u64 {
        (val + align - 1) / align * align
    }
}

pub struct ElfWriter {
    arch: Arch,
    is_64: bool,
    is_rela: bool,
    config: ElfWriterConfig,
}

impl ElfWriter {
    /// Create a new ELF writer with default configuration
    pub fn new(arch: Arch) -> Self {
        let config = ElfWriterConfig::default();
        let (is_64, is_rela) = match arch {
            Arch::X86_64 => (true, true),
            Arch::Aarch64 => (true, true),
            Arch::Riscv64 => (true, true),
            Arch::Riscv32 => (false, true),
            Arch::Arm => (false, false),
        };
        Self {
            arch,
            is_64,
            is_rela,
            config,
        }
    }

    /// Create a new ELF writer with custom configuration
    pub fn with_config(arch: Arch, config: ElfWriterConfig) -> Self {
        let (is_64, is_rela) = match arch {
            Arch::X86_64 => (true, true),
            Arch::Aarch64 => (true, true),
            Arch::Riscv64 => (true, true),
            Arch::Riscv32 => (false, true),
            Arch::Arm => (false, false),
        };
        Self {
            arch,
            is_64,
            is_rela,
            config,
        }
    }

    /// Write ELF to file, returning the output metadata for verification
    pub fn write(
        &self,
        out_path: &Path,
        relocs: &[RelocEntry],
        symbols: &[SymbolDesc],
    ) -> Result<ElfWriteOutput> {
        let output = self.write_elf(relocs, symbols)?;
        std::fs::write(out_path, &output.data)?;
        Ok(output)
    }

    /// Generate ELF with relocation information.
    ///
    /// This is the main method that orchestrates the entire ELF generation process:
    /// 1. Build text and data segments
    /// 2. Calculate memory layout
    /// 3. Generate dynamic tables (strings, symbols, hash)
    /// 4. Process relocations and collect metadata
    /// 5. Write complete ELF file structure
    ///
    /// # Arguments
    ///
    /// * `relocs` - Array of relocation entries to be applied
    /// * `symbols` - Array of symbol descriptors to include in the symbol table
    ///
    /// # Returns
    ///
    /// An `ElfWriteOutput` containing:
    /// - `data`: Raw ELF file bytes ready to be written to disk
    /// - `base_addr`: Base address used during generation
    /// - `data_vaddr`: Virtual address of the data segment
    /// - `text_vaddr`: Virtual address of the text segment
    /// - `relocations`: Metadata about each relocation for testing
    ///
    /// # Example
    ///
    /// ```ignore
    /// let output = writer.write_elf(&relocs, &symbols)?;
    /// std::fs::write("output.so", &output.data)?;
    /// ```
    pub fn write_elf(
        &self,
        relocs: &[RelocEntry],
        symbols: &[SymbolDesc],
    ) -> Result<ElfWriteOutput> {
        let text_blob = self.build_text_blob();
        let mut data_blob = vec![0u8; self.config.initial_data_size as usize];

        // Initialize data segment
        self.init_data_segment(&mut data_blob, symbols);

        // Calculate layout and segment positions
        let (layout, text_vaddr, _text_off, _text_size, _data_off, _data_size, data_vaddr) =
            self.calculate_layout(&text_blob, &data_blob);

        // Build dynamic sections
        let (_names, dynstr_blob, dynsym_blob, hash_blob, sym_name_to_idx) =
            self.build_dyn_tables(symbols)?;

        // Build relocations and collect metadata
        let (rela_dyn_blob, rela_plt_blob, relative_count, reloc_infos) = self
            .build_relocations_with_metadata(
                relocs,
                data_vaddr,
                &mut data_blob,
                &sym_name_to_idx,
            )?;

        // Build section string table
        let shstr = self.build_section_string_table();

        // Calculate remaining layout for file writing
        let layout = self.complete_layout(
            layout,
            &dynstr_blob,
            &dynsym_blob,
            &hash_blob,
            &rela_dyn_blob,
            &rela_plt_blob,
            &shstr,
        );

        // Build .dynamic section
        let dynamic_blob = self.build_dynamic_section(
            data_vaddr,
            &dynstr_blob,
            &dynsym_blob,
            &hash_blob,
            &rela_dyn_blob,
            &rela_plt_blob,
            relative_count,
        )?;

        // Write ELF file
        let data = self.write_elf_file(
            layout,
            &text_blob,
            &data_blob,
            &dynstr_blob,
            &dynsym_blob,
            &hash_blob,
            &rela_dyn_blob,
            &rela_plt_blob,
            &dynamic_blob,
            &shstr,
        )?;

        Ok(ElfWriteOutput {
            data,
            base_addr: self.config.base_addr,
            data_vaddr,
            text_vaddr,
            relocations: reloc_infos,
        })
    }

    fn init_data_segment(&self, data_blob: &mut [u8], symbols: &[SymbolDesc]) {
        for s in symbols {
            let name = &s.name;
            let addr = if name == crate::LOCAL_VAR_NAME {
                crate::LOCAL_VAR_OFF
            } else if name == EXTERNAL_VAR_NAME {
                0
            } else {
                0
            };
            let addr = addr as usize;
            if addr + 8 <= data_blob.len() {
                data_blob[addr..addr + 8].copy_from_slice(&(addr as u64).to_le_bytes());
            }
        }

        // Initialize GOT table at offset 0x100 (arbitrary location in data segment)
        // GOT[0] = address of _DYNAMIC (will be filled by loader)
        // GOT[1] = link_map pointer (will be filled by loader)
        // GOT[2] = _dl_runtime_resolve address (will be filled by loader)
        // GOT[3+] = PLT function entries (initially point to PLT stub)
        let got_offset = 0x100usize;
        let num_got_entries = 8; // 3 reserved + 5 for PLT functions
        if got_offset + num_got_entries * 8 <= data_blob.len() {
            // Reserve space for GOT entries
            for i in 0..num_got_entries {
                let offset = got_offset + i * 8;
                if offset + 8 <= data_blob.len() {
                    // For PLT entries (index >= 3), initialize with address of PLT stub
                    // This will be updated by the loader on first call (lazy binding)
                    data_blob[offset..offset + 8].fill(0);
                }
            }
        }
    }

    fn calculate_layout(
        &self,
        text_blob: &[u8],
        data_blob: &[u8],
    ) -> (ElfLayout, u64, u64, u64, u64, u64, u64) {
        let ehdr_size = if self.is_64 {
            EHDR_SIZE_64
        } else {
            EHDR_SIZE_32
        };
        let phentsize = if self.is_64 {
            PHDR_SIZE_64
        } else {
            PHDR_SIZE_32
        };
        let phnum = NUM_PROGRAM_HEADERS;
        let ph_size = phentsize * phnum;

        let mut layout = ElfLayout::new(
            self.config.base_addr,
            self.config.page_size,
            self.config.section_align,
        );
        layout.add_header(ehdr_size, ph_size);

        let text_size = text_blob.len() as u64;
        let (text_off, text_vaddr) = layout.add_segment(text_size);

        let data_size = data_blob.len() as u64;
        let (data_off, data_vaddr) = layout.add_segment(data_size);

        (
            layout, text_vaddr, text_off, text_size, data_off, data_size, data_vaddr,
        )
    }

    fn build_section_string_table(&self) -> Vec<u8> {
        let mut shstr = vec![0u8];
        let sect_names = [
            ".text",
            ".data",
            ".dynstr",
            ".dynsym",
            if self.is_rela {
                ".rela.dyn"
            } else {
                ".rel.dyn"
            },
            if self.is_rela {
                ".rela.plt"
            } else {
                ".rel.plt"
            },
            ".dynamic",
            ".hash",
            ".shstrtab",
        ];
        for n in &sect_names {
            shstr.extend_from_slice(n.as_bytes());
            shstr.push(0);
        }
        shstr
    }

    fn complete_layout(
        &self,
        mut layout: ElfLayout,
        dynstr_blob: &[u8],
        dynsym_blob: &[u8],
        hash_blob: &[u8],
        rela_dyn_blob: &[u8],
        rela_plt_blob: &[u8],
        shstr: &[u8],
    ) -> ElfLayout {
        let align = layout.section_align;
        layout.add_section(dynstr_blob.len() as u64);
        layout.add_section(dynsym_blob.len() as u64);
        layout.add_section(hash_blob.len() as u64);
        layout.add_section(rela_dyn_blob.len() as u64);
        layout.add_section(rela_plt_blob.len() as u64);
        layout.add_section(0); // dynamic
        layout.file_off = layout.align_up(layout.file_off + shstr.len() as u64, align);
        layout
    }

    fn build_dynamic_section(
        &self,
        data_vaddr: u64,
        dynstr_blob: &[u8],
        dynsym_blob: &[u8],
        hash_blob: &[u8],
        rela_dyn_blob: &[u8],
        rela_plt_blob: &[u8],
        relative_count: u64,
    ) -> Result<Vec<u8>> {
        // Calculate virtual addresses (relative to base)
        // Need to align each section similar to how we do in file layout
        let align = self.config.section_align;
        let mut vaddr_offset = data_vaddr + self.config.initial_data_size;

        let dynstr_vaddr = vaddr_offset;
        vaddr_offset = self.align_up(vaddr_offset + dynstr_blob.len() as u64, align);

        let dynsym_vaddr = vaddr_offset;
        vaddr_offset = self.align_up(vaddr_offset + dynsym_blob.len() as u64, align);

        let hash_vaddr = vaddr_offset;
        vaddr_offset = self.align_up(vaddr_offset + hash_blob.len() as u64, align);

        let rela_dyn_vaddr = vaddr_offset;
        vaddr_offset = self.align_up(vaddr_offset + rela_dyn_blob.len() as u64, align);

        let rela_plt_vaddr = vaddr_offset;

        // GOT table is at offset 0x100 in data segment
        let got_vaddr = data_vaddr + 0x100;

        self.build_dynamic_blob(
            dynstr_vaddr,
            dynstr_blob.len() as u64,
            dynsym_vaddr,
            dynsym_blob.len() as u64,
            hash_vaddr,
            rela_dyn_vaddr,
            rela_dyn_blob.len() as u64,
            rela_plt_vaddr,
            rela_plt_blob.len() as u64,
            relative_count,
            got_vaddr,
        )
    }

    fn write_elf_file(
        &self,
        layout: ElfLayout,
        text_blob: &[u8],
        data_blob: &[u8],
        dynstr_blob: &[u8],
        dynsym_blob: &[u8],
        hash_blob: &[u8],
        rela_dyn_blob: &[u8],
        rela_plt_blob: &[u8],
        dynamic_blob: &[u8],
        shstr: &[u8],
    ) -> Result<Vec<u8>> {
        let mut out_bytes = vec![];
        let align = layout.section_align;
        let ehdr_size = if self.is_64 {
            EHDR_SIZE_64
        } else {
            EHDR_SIZE_32
        };
        let phentsize = if self.is_64 {
            PHDR_SIZE_64
        } else {
            PHDR_SIZE_32
        };
        let shentsize = if self.is_64 {
            SHDR_SIZE_64
        } else {
            SHDR_SIZE_32
        };

        // Calculate section offsets
        let mut sec_layout = ElfLayout::new(self.config.base_addr, self.config.page_size, align);
        let ehdr_ph_size = ehdr_size + phentsize * NUM_PROGRAM_HEADERS;
        sec_layout.file_off = sec_layout.align_up(ehdr_ph_size, self.config.page_size);

        let text_off = sec_layout.file_off;
        sec_layout.file_off = sec_layout.align_up(
            sec_layout.file_off + text_blob.len() as u64,
            self.config.page_size,
        );

        let data_off = sec_layout.file_off;
        sec_layout.file_off = sec_layout.align_up(
            sec_layout.file_off + data_blob.len() as u64,
            self.config.page_size,
        );

        let dynstr_off = sec_layout.file_off;
        sec_layout.file_off =
            sec_layout.align_up(sec_layout.file_off + dynstr_blob.len() as u64, align);

        let dynsym_off = sec_layout.file_off;
        sec_layout.file_off =
            sec_layout.align_up(sec_layout.file_off + dynsym_blob.len() as u64, align);

        let hash_off = sec_layout.file_off;
        sec_layout.file_off =
            sec_layout.align_up(sec_layout.file_off + hash_blob.len() as u64, align);

        let rela_dyn_off = sec_layout.file_off;
        sec_layout.file_off =
            sec_layout.align_up(sec_layout.file_off + rela_dyn_blob.len() as u64, align);

        let rela_plt_off = sec_layout.file_off;
        sec_layout.file_off =
            sec_layout.align_up(sec_layout.file_off + rela_plt_blob.len() as u64, align);

        let dynamic_off = sec_layout.file_off;
        sec_layout.file_off =
            sec_layout.align_up(sec_layout.file_off + dynamic_blob.len() as u64, align);

        let shstr_off = sec_layout.file_off;
        sec_layout.file_off = sec_layout.align_up(sec_layout.file_off + shstr.len() as u64, align);
        let shoff = sec_layout.file_off;

        let sect_names = [
            ".text",
            ".data",
            ".dynstr",
            ".dynsym",
            if self.is_rela {
                ".rela.dyn"
            } else {
                ".rel.dyn"
            },
            if self.is_rela {
                ".rela.plt"
            } else {
                ".rel.plt"
            },
            ".dynamic",
            ".hash",
            ".shstrtab",
        ];

        let shnum = (sect_names.len() + 1) as u16;
        let shstrndx = sect_names.len() as u16;

        // Write ELF Header
        let mut ident = [0u8; ELF_IDENT_SIZE];
        ident[0] = ELF_IDENT_MAG0;
        ident[1] = ELF_IDENT_MAG1;
        ident[2] = ELF_IDENT_MAG2;
        ident[3] = ELF_IDENT_MAG3;
        ident[ELF_IDENT_CLASS_OFFSET] = if self.is_64 { ELFCLASS64 } else { ELFCLASS32 };
        ident[ELF_IDENT_DATA_OFFSET] = ELFDATA2LSB;
        ident[ELF_IDENT_VERSION_OFFSET] = EV_CURRENT;
        ident[ELF_IDENT_OSABI_OFFSET] = ELFOSABI_SYSV;

        let ehdr = ElfHeader {
            ident,
            type_: ET_DYN,
            machine: self.get_machine(),
            version: EV_CURRENT as u32,
            entry: 0,
            phoff: ehdr_size,
            shoff,
            flags: 0,
            ehsize: ehdr_size as u16,
            phentsize: phentsize as u16,
            phnum: NUM_PROGRAM_HEADERS as u16,
            shentsize: shentsize as u16,
            shnum,
            shstrndx,
        };
        self.write_struct(&mut out_bytes, ehdr)?;

        // Write Program Headers
        let text_vaddr = self.config.base_addr + text_off;
        let data_vaddr = self.config.base_addr + data_off;
        let dynamic_vaddr = self.config.base_addr + dynamic_off;
        let dynamic_size = dynamic_blob.len() as u64;

        // Text segment (RX)
        self.write_phdr(
            &mut out_bytes,
            PT_LOAD,
            PF_R | PF_X,
            text_off,
            text_vaddr,
            text_blob.len() as u64,
            self.config.page_size,
        )?;

        // Data segment (RW) - includes everything up to end of dynamic
        let data_segment_size = (dynamic_off + dynamic_size) - data_off;
        self.write_phdr(
            &mut out_bytes,
            PT_LOAD,
            PF_R | PF_W,
            data_off,
            data_vaddr,
            data_segment_size,
            self.config.page_size,
        )?;

        // Dynamic segment
        self.write_phdr(
            &mut out_bytes,
            PT_DYNAMIC,
            PF_R | PF_W,
            dynamic_off,
            dynamic_vaddr,
            dynamic_size,
            align,
        )?;

        // Write sections
        self.write_at(&mut out_bytes, text_off, text_blob);
        self.write_at(&mut out_bytes, data_off, data_blob);
        self.write_at(&mut out_bytes, dynstr_off, dynstr_blob);
        self.write_at(&mut out_bytes, dynsym_off, dynsym_blob);
        self.write_at(&mut out_bytes, rela_dyn_off, rela_dyn_blob);
        self.write_at(&mut out_bytes, rela_plt_off, rela_plt_blob);
        self.write_at(&mut out_bytes, hash_off, hash_blob);
        self.write_at(&mut out_bytes, dynamic_off, dynamic_blob);
        self.write_at(&mut out_bytes, shstr_off, shstr);

        // Write Section Headers
        if out_bytes.len() < shoff as usize {
            out_bytes.resize(shoff as usize, 0);
        }

        // Null section
        self.write_shdr(&mut out_bytes, 0, SHT_NULL, 0, 0, 0, 0, 0, 0, 0)?;

        for name in &sect_names {
            let name_bytes = name.as_bytes();
            let name_off = shstr
                .windows(name_bytes.len())
                .position(|w| w == name_bytes)
                .unwrap_or(0) as u32;

            let shtype = match *name {
                ".text" | ".data" => SHT_PROGBITS,
                ".dynstr" | ".shstrtab" => SHT_STRTAB,
                ".dynsym" => SHT_DYNSYM,
                ".rela.dyn" | ".rela.plt" => SHT_RELA,
                ".rel.dyn" | ".rel.plt" => SHT_REL,
                ".dynamic" => SHT_DYNAMIC,
                ".hash" => SHT_HASH,
                _ => SHT_PROGBITS,
            };

            let flags = match *name {
                ".text" => SHF_ALLOC | SHF_EXECINSTR,
                ".data" => SHF_ALLOC | SHF_WRITE,
                ".dynstr" | ".dynsym" | ".rela.dyn" | ".rela.plt" | ".rel.dyn" | ".rel.plt"
                | ".dynamic" | ".hash" => SHF_ALLOC,
                _ => 0,
            };

            let (off, sz) = match *name {
                ".text" => (text_off, text_blob.len() as u64),
                ".data" => (data_off, data_blob.len() as u64),
                ".dynstr" => (dynstr_off, dynstr_blob.len() as u64),
                ".dynsym" => (dynsym_off, dynsym_blob.len() as u64),
                ".rela.dyn" | ".rel.dyn" => (rela_dyn_off, rela_dyn_blob.len() as u64),
                ".rela.plt" | ".rel.plt" => (rela_plt_off, rela_plt_blob.len() as u64),
                ".dynamic" => (dynamic_off, dynamic_blob.len() as u64),
                ".shstrtab" => (shstr_off, shstr.len() as u64),
                ".hash" => (hash_off, hash_blob.len() as u64),
                _ => (0, 0),
            };

            let entsize = match shtype {
                SHT_DYNSYM => {
                    if self.is_64 {
                        SYM_SIZE_64
                    } else {
                        SYM_SIZE_32
                    }
                }
                SHT_RELA => {
                    if self.is_64 {
                        RELA_SIZE_64
                    } else {
                        RELA_SIZE_32
                    }
                }
                SHT_REL => {
                    if self.is_64 {
                        REL_SIZE_64
                    } else {
                        REL_SIZE_32
                    }
                }
                SHT_DYNAMIC => {
                    if self.is_64 {
                        DYN_SIZE_64
                    } else {
                        DYN_SIZE_32
                    }
                }
                SHT_HASH => HASH_SIZE,
                _ => 0,
            };

            let (link, info) = match *name {
                ".dynsym" => (3, 1),
                ".rela.dyn" | ".rel.dyn" => (4, 0),
                ".rela.plt" | ".rel.plt" => (4, 0),
                ".dynamic" => (3, 0),
                ".hash" => (4, 0),
                _ => (0, 0),
            };

            self.write_shdr(
                &mut out_bytes,
                name_off,
                shtype,
                flags as u64,
                self.config.base_addr + off,
                off,
                sz,
                link,
                info,
                entsize,
            )?;
        }

        Ok(out_bytes)
    }

    fn write_at(&self, buf: &mut Vec<u8>, offset: u64, data: &[u8]) {
        let offset = offset as usize;
        if buf.len() < offset + data.len() {
            buf.resize(offset + data.len(), 0);
        }
        buf.splice(offset..offset + data.len(), data.iter().cloned());
    }

    fn write_dyn_tag(&self, buf: &mut Vec<u8>, tag: u64, val: u64) -> Result<()> {
        if self.is_64 {
            buf.write_u64::<LittleEndian>(tag)?;
            buf.write_u64::<LittleEndian>(val)?;
        } else {
            buf.write_u32::<LittleEndian>(tag as u32)?;
            buf.write_u32::<LittleEndian>(val as u32)?;
        }
        Ok(())
    }

    // Build the .dynamic section blob from known section virtual addresses/sizes
    fn build_dynamic_blob(
        &self,
        dynstr_vaddr: u64,
        dynstr_size: u64,
        dynsym_vaddr: u64,
        _dynsym_size: u64,
        hash_vaddr: u64,
        rela_dyn_vaddr: u64,
        rela_dyn_size: u64,
        rela_plt_vaddr: u64,
        rela_plt_size: u64,
        relative_count: u64,
        got_vaddr: u64,
    ) -> Result<Vec<u8>> {
        let mut dynamic_blob: Vec<u8> = vec![];

        self.write_dyn_tag(&mut dynamic_blob, DT_STRTAB as u64, dynstr_vaddr)?;
        self.write_dyn_tag(&mut dynamic_blob, DT_STRSZ as u64, dynstr_size)?;
        self.write_dyn_tag(&mut dynamic_blob, DT_SYMTAB as u64, dynsym_vaddr)?;
        self.write_dyn_tag(
            &mut dynamic_blob,
            DT_SYMENT as u64,
            if self.is_64 { SYM_SIZE_64 } else { SYM_SIZE_32 },
        )?;
        // Add DT_HASH to satisfy loader
        self.write_dyn_tag(&mut dynamic_blob, DT_HASH as u64, hash_vaddr)?;
        // Add DT_PLTGOT for GOT table support
        self.write_dyn_tag(&mut dynamic_blob, DT_PLTGOT as u64, got_vaddr)?;

        if rela_dyn_size > 0 {
            if self.is_rela {
                self.write_dyn_tag(&mut dynamic_blob, DT_RELA as u64, rela_dyn_vaddr)?;
                self.write_dyn_tag(&mut dynamic_blob, DT_RELASZ as u64, rela_dyn_size)?;
                self.write_dyn_tag(
                    &mut dynamic_blob,
                    DT_RELAENT as u64,
                    if self.is_64 {
                        RELA_SIZE_64
                    } else {
                        RELA_SIZE_32
                    },
                )?;
                if relative_count > 0 {
                    self.write_dyn_tag(&mut dynamic_blob, DT_RELACOUNT as u64, relative_count)?;
                }
            } else {
                self.write_dyn_tag(&mut dynamic_blob, DT_REL as u64, rela_dyn_vaddr)?;
                self.write_dyn_tag(&mut dynamic_blob, DT_RELSZ as u64, rela_dyn_size)?;
                self.write_dyn_tag(
                    &mut dynamic_blob,
                    DT_RELENT as u64,
                    if self.is_64 { REL_SIZE_64 } else { REL_SIZE_32 },
                )?;
                if relative_count > 0 {
                    self.write_dyn_tag(&mut dynamic_blob, DT_RELCOUNT as u64, relative_count)?;
                }
            }
        }

        if rela_plt_size > 0 {
            self.write_dyn_tag(&mut dynamic_blob, DT_JMPREL as u64, rela_plt_vaddr)?;
            self.write_dyn_tag(&mut dynamic_blob, DT_PLTRELSZ as u64, rela_plt_size)?;
            self.write_dyn_tag(
                &mut dynamic_blob,
                DT_PLTREL as u64,
                if self.is_rela {
                    DT_RELA as u64
                } else {
                    DT_REL as u64
                },
            )?;
        }

        self.write_dyn_tag(&mut dynamic_blob, DT_NULL as u64, 0)?;

        Ok(dynamic_blob)
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

    fn is_plt_reloc(&self, r_type: u64) -> bool {
        match self.arch {
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

    fn is_relative_reloc(&self, r_type: u64) -> bool {
        match self.arch {
            Arch::X86_64 => r_type == R_X86_64_RELATIVE as u64,
            Arch::Aarch64 => r_type == R_AARCH64_RELATIVE as u64,
            Arch::Riscv64 | Arch::Riscv32 => r_type == R_RISCV_RELATIVE as u64,
            Arch::Arm => r_type == R_ARM_RELATIVE as u64,
        }
    }
}

struct RelocationEntry {
    offset: u64,
    info: u64,
    addend: i64,
    is_rela: bool,
}

/// Describes a symbol with explicit attributes for ELF symbol table.
/// All properties must be configured before use in build_dynsym.
///
/// SymbolDesc is used to define symbols that will appear in the dynamic symbol table.
/// Symbols can be:
/// - Global functions (undefined, to be resolved at runtime)
/// - Global data objects (undefined, to be resolved at runtime)
/// - Locally defined symbols (in specific sections)
pub struct SymbolDesc {
    pub name: String,
    pub st_info: u8,
    pub shndx: u16,
    pub value: u64,
}

impl SymbolDesc {
    /// Create a global function symbol (undefined, external).
    ///
    /// This creates a symbol that will be resolved by the dynamic linker at runtime.
    /// # Example
    /// ```ignore
    /// let sym = SymbolDesc::global_func("malloc");
    /// ```
    pub fn global_func(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            st_info: ((STB_GLOBAL << 4) | STT_FUNC) as u8,
            shndx: SHN_UNDEF as u16,
            value: 0,
        }
    }

    /// Create a global data object symbol (undefined, external).
    ///
    /// This creates a symbol for external data that will be resolved by the dynamic linker.
    /// # Example
    /// ```ignore
    /// let sym = SymbolDesc::global_object("errno");
    /// ```
    pub fn global_object(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            st_info: ((STB_GLOBAL << 4) | STT_OBJECT) as u8,
            shndx: SHN_UNDEF as u16,
            value: 0,
        }
    }

    /// Create a locally defined symbol (in specified section).
    ///
    /// This creates a symbol that is defined within the ELF file itself (e.g., in .data section).
    /// # Example
    /// ```ignore
    /// let sym = SymbolDesc::local_var("my_var", 0x20); // at offset 0x20
    /// ```
    pub fn local_var(name: impl Into<String>, section_offset: u16) -> Self {
        Self {
            name: name.into(),
            st_info: ((STB_GLOBAL << 4) | STT_OBJECT) as u8,
            shndx: section_offset,
            value: 0,
        }
    }
}

struct Symbol {
    name: u32,
    info: u8,
    other: u8,
    shndx: u16,
    value: u64,
    size: u64,
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

struct ProgramHeader {
    type_: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    paddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

struct SectionHeader {
    name: u32,
    type_: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    addralign: u64,
    entsize: u64,
}

trait WriteToBuf {
    fn write(&self, buf: &mut Vec<u8>, is_64: bool) -> Result<()>;
}

impl WriteToBuf for RelocationEntry {
    fn write(&self, buf: &mut Vec<u8>, is_64: bool) -> Result<()> {
        if is_64 {
            buf.write_u64::<LittleEndian>(self.offset)?;
            buf.write_u64::<LittleEndian>(self.info)?;
            if self.is_rela {
                buf.write_i64::<LittleEndian>(self.addend)?;
            }
        } else {
            buf.write_u32::<LittleEndian>(self.offset as u32)?;
            buf.write_u32::<LittleEndian>(self.info as u32)?;
            if self.is_rela {
                buf.write_i32::<LittleEndian>(self.addend as i32)?;
            }
        }
        Ok(())
    }
}

impl WriteToBuf for Symbol {
    fn write(&self, buf: &mut Vec<u8>, is_64: bool) -> Result<()> {
        if is_64 {
            buf.write_u32::<LittleEndian>(self.name)?;
            buf.write_u8(self.info)?;
            buf.write_u8(self.other)?;
            buf.write_u16::<LittleEndian>(self.shndx)?;
            buf.write_u64::<LittleEndian>(self.value)?;
            buf.write_u64::<LittleEndian>(self.size)?;
        } else {
            buf.write_u32::<LittleEndian>(self.name)?;
            buf.write_u32::<LittleEndian>(self.value as u32)?;
            buf.write_u32::<LittleEndian>(self.size as u32)?;
            buf.write_u8(self.info)?;
            buf.write_u8(self.other)?;
            buf.write_u16::<LittleEndian>(self.shndx)?;
        }
        Ok(())
    }
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

impl WriteToBuf for ProgramHeader {
    fn write(&self, buf: &mut Vec<u8>, is_64: bool) -> Result<()> {
        if is_64 {
            buf.write_u32::<LittleEndian>(self.type_)?;
            buf.write_u32::<LittleEndian>(self.flags)?;
            buf.write_u64::<LittleEndian>(self.offset)?;
            buf.write_u64::<LittleEndian>(self.vaddr)?;
            buf.write_u64::<LittleEndian>(self.paddr)?;
            buf.write_u64::<LittleEndian>(self.filesz)?;
            buf.write_u64::<LittleEndian>(self.memsz)?;
            buf.write_u64::<LittleEndian>(self.align)?;
        } else {
            buf.write_u32::<LittleEndian>(self.type_)?;
            buf.write_u32::<LittleEndian>(self.offset as u32)?;
            buf.write_u32::<LittleEndian>(self.vaddr as u32)?;
            buf.write_u32::<LittleEndian>(self.paddr as u32)?;
            buf.write_u32::<LittleEndian>(self.filesz as u32)?;
            buf.write_u32::<LittleEndian>(self.memsz as u32)?;
            buf.write_u32::<LittleEndian>(self.flags)?;
            buf.write_u32::<LittleEndian>(self.align as u32)?;
        }
        Ok(())
    }
}

impl WriteToBuf for SectionHeader {
    fn write(&self, buf: &mut Vec<u8>, is_64: bool) -> Result<()> {
        if is_64 {
            buf.write_u32::<LittleEndian>(self.name)?;
            buf.write_u32::<LittleEndian>(self.type_)?;
            buf.write_u64::<LittleEndian>(self.flags)?;
            buf.write_u64::<LittleEndian>(self.addr)?;
            buf.write_u64::<LittleEndian>(self.offset)?;
            buf.write_u64::<LittleEndian>(self.size)?;
            buf.write_u32::<LittleEndian>(self.link)?;
            buf.write_u32::<LittleEndian>(self.info)?;
            buf.write_u64::<LittleEndian>(self.addralign)?;
            buf.write_u64::<LittleEndian>(self.entsize)?;
        } else {
            buf.write_u32::<LittleEndian>(self.name)?;
            buf.write_u32::<LittleEndian>(self.type_)?;
            buf.write_u32::<LittleEndian>(self.flags as u32)?;
            buf.write_u32::<LittleEndian>(self.addr as u32)?;
            buf.write_u32::<LittleEndian>(self.offset as u32)?;
            buf.write_u32::<LittleEndian>(self.size as u32)?;
            buf.write_u32::<LittleEndian>(self.link)?;
            buf.write_u32::<LittleEndian>(self.info)?;
            buf.write_u32::<LittleEndian>(self.addralign as u32)?;
            buf.write_u32::<LittleEndian>(self.entsize as u32)?;
        }
        Ok(())
    }
}

fn align_of_section(type_: u32) -> u64 {
    match type_ {
        SHT_PROGBITS => 16,
        SHT_SYMTAB => 8,
        SHT_STRTAB => 1,
        SHT_RELA => 8,
        SHT_HASH => 8,
        SHT_DYNAMIC => 8,
        SHT_NOTE => 4,
        SHT_NOBITS => 32,
        SHT_REL => 8,
        SHT_DYNSYM => 8,
        _ => 1,
    }
}

impl ElfWriter {
    // ... existing methods ...

    fn align_up(&self, val: u64, align: u64) -> u64 {
        (val + align - 1) / align * align
    }

    fn write_struct<T: WriteToBuf>(&self, buf: &mut Vec<u8>, item: T) -> Result<()> {
        item.write(buf, self.is_64)
    }

    fn write_phdr(
        &self,
        buf: &mut Vec<u8>,
        type_: u32,
        flags: u32,
        offset: u64,
        vaddr: u64,
        filesz: u64,
        align: u64,
    ) -> Result<()> {
        let phdr = ProgramHeader {
            type_,
            flags,
            offset,
            vaddr,
            paddr: vaddr,
            filesz,
            memsz: filesz,
            align,
        };
        self.write_struct(buf, phdr)
    }

    fn write_shdr(
        &self,
        buf: &mut Vec<u8>,
        name: u32,
        type_: u32,
        flags: u64,
        addr: u64,
        offset: u64,
        size: u64,
        link: u32,
        info: u32,
        entsize: u64,
    ) -> Result<()> {
        let shdr = SectionHeader {
            name,
            type_,
            flags,
            addr,
            offset,
            size,
            link,
            info,
            addralign: align_of_section(type_),
            entsize,
        };
        self.write_struct(buf, shdr)
    }

    // Build a small resolver in .text used for IFUNC tests
    fn build_text_blob(&self) -> Vec<u8> {
        let mut text_data = vec![];

        // Build PLT (Procedure Linkage Table) for lazy binding support
        if self.arch == Arch::X86_64 {
            // PLT[0]: Special entry for dynamic linker
            // push qword [GOT+8]   ; push link_map
            // jmp qword [GOT+16]   ; jump to _dl_runtime_resolve
            text_data.extend_from_slice(&[
                0xff, 0x35, 0x00, 0x00, 0x00,
                0x00, // push qword [rip+offset] - will need reloc
                0xff, 0x25, 0x00, 0x00, 0x00,
                0x00, // jmp qword [rip+offset] - will need reloc
            ]);
            // Padding to 16 bytes
            text_data.resize(16, 0x90);

            // PLT[1]: Entry for external_func
            // jmp qword [GOT+n]
            // push index
            // jmp PLT[0]
            text_data.extend_from_slice(&[
                0xff, 0x25, 0x00, 0x00, 0x00, 0x00, // jmp qword [rip+offset] - GOT entry
                0x68, 0x00, 0x00, 0x00, 0x00, // push 0 (relocation index)
                0xe9, 0x00, 0x00, 0x00, 0x00, // jmp PLT[0]
            ]);

            // Additional function stub
            text_data.extend_from_slice(&[0x48, 0xb8]);
            text_data.extend_from_slice(&0x1000u64.to_le_bytes());
            text_data.push(0xc3);
        } else {
            // Fallback: fill with NOPs and a RET
            text_data.extend_from_slice(&vec![0x90u8; 64]);
            text_data.push(0xc3);
        }

        // Ensure minimum size
        if text_data.len() < 64 {
            text_data.resize(64, 0x90);
        }
        text_data
    }

    // Build dynamic string table and name list from symbol descriptors
    fn build_names_and_dynstr(
        &self,
        symbols: &[SymbolDesc],
    ) -> (Vec<String>, Vec<u8>, HashMap<String, u32>) {
        let mut dynstr = vec![0u8]; // null
        let names: Vec<String> = symbols.iter().map(|s| s.name.clone()).collect();
        let mut name_index = HashMap::new();
        for name in &names {
            name_index.insert(name.clone(), dynstr.len() as u32);
            dynstr.extend_from_slice(name.as_bytes());
            dynstr.push(0);
        }
        (names, dynstr, name_index)
    }

    // Build dynamic string table, dynamic symbol table (including initial NULL entry),
    // and a simple SYSV hash table. Returns (names, dynstr_blob, dynsym_blob, hash_blob, sym_name_to_idx).
    fn build_dyn_tables(
        &self,
        symbols: &[SymbolDesc],
    ) -> Result<(Vec<String>, Vec<u8>, Vec<u8>, Vec<u8>, HashMap<String, u64>)> {
        // Reuse helper to build dynstr and name index
        let (names, dynstr, name_index) = self.build_names_and_dynstr(symbols);

        // dynsym: start with NULL symbol
        let mut dynsym = vec![];
        let sym_entry_size = if self.is_64 {
            SYM_SIZE_64 as usize
        } else {
            SYM_SIZE_32 as usize
        };
        dynsym.extend_from_slice(&vec![0u8; sym_entry_size]); // Null symbol

        // Append other symbols (skip local var symbol) and get symbol index mapping
        let (extra, sym_name_to_idx) = self.build_dynsym(symbols, &name_index)?;
        dynsym.extend_from_slice(&extra);

        // Simple SYSV hash: nbucket=1, nchain=num_syms
        let num_syms = (names.len() + 1) as u32; // +1 for null
        let mut hash_blob = vec![];
        hash_blob.write_u32::<LittleEndian>(1)?; // nbucket
        hash_blob.write_u32::<LittleEndian>(num_syms)?; // nchain
                                                        // bucket[0]
        if num_syms > 1 {
            hash_blob.write_u32::<LittleEndian>(1)?;
        } else {
            hash_blob.write_u32::<LittleEndian>(0)?;
        }
        // chain[0]
        hash_blob.write_u32::<LittleEndian>(0)?;
        for i in 1..num_syms {
            if i < num_syms - 1 {
                hash_blob.write_u32::<LittleEndian>(i + 1)?;
            } else {
                hash_blob.write_u32::<LittleEndian>(0)?;
            }
        }

        Ok((names, dynstr, dynsym, hash_blob, sym_name_to_idx))
    }

    /// Build dynamic symbol table entries (except initial NULL already reserved)
    /// Returns (dynsym_blob, symbol_name_to_index_map)
    fn build_dynsym(
        &self,
        symbols: &[SymbolDesc],
        name_index: &HashMap<String, u32>,
    ) -> Result<(Vec<u8>, HashMap<String, u64>)> {
        let mut dynsym = vec![];
        let mut sym_name_to_idx = HashMap::new();
        let mut current_idx = 1u64; // Start from 1 (0 is NULL symbol)

        for s in symbols {
            if s.name == crate::LOCAL_VAR_NAME {
                continue;
            }

            let name_idx = name_index
                .get(&s.name)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("Symbol {} not found in name index", s.name))?;

            // Record symbol name to dynsym index mapping
            sym_name_to_idx.insert(s.name.clone(), current_idx);
            current_idx += 1;

            let sym = Symbol {
                name: name_idx,
                info: s.st_info,
                other: 0,
                shndx: s.shndx,
                value: s.value,
                size: 0,
            };
            self.write_struct(&mut dynsym, sym)?;
        }
        Ok((dynsym, sym_name_to_idx))
    }

    // Build relocation blobs; may mutate data_blob to write implicit addends for REL format
    fn build_relocations_with_metadata(
        &self,
        relocs: &[RelocEntry],
        data_vaddr: u64,
        data_blob: &mut [u8],
        sym_name_to_idx: &HashMap<String, u64>,
    ) -> Result<(Vec<u8>, Vec<u8>, u64, Vec<RelocationInfo>)> {
        let mut rela_dyn = vec![];
        let mut rela_plt = vec![];
        let mut reloc_infos = vec![];

        let mut sorted_relocs: Vec<&RelocEntry> = vec![];
        for r in relocs {
            let r_type = match r.flags {
                object::write::RelocationFlags::Elf { r_type } => r_type as u64,
                _ => unreachable!(),
            };
            if self.is_relative_reloc(r_type) {
                sorted_relocs.push(r);
            }
        }
        let relative_count = sorted_relocs.len() as u64;

        for r in relocs {
            let r_type = match r.flags {
                object::write::RelocationFlags::Elf { r_type } => r_type as u64,
                _ => 0u64,
            };
            if !self.is_relative_reloc(r_type) {
                sorted_relocs.push(r);
            }
        }

        for r in sorted_relocs {
            let r_type = match r.flags {
                object::write::RelocationFlags::Elf { r_type } => r_type as u64,
                _ => 0u64,
            };

            let sym_idx = if self.is_relative_reloc(r_type) {
                0
            } else {
                // Look up symbol index from the mapping built during dynsym generation
                // Use empty string for RELATIVE/IRELATIVE that have no symbol
                let sym_name = if r.symbol_name.is_empty() {
                    ""
                } else {
                    &r.symbol_name
                };
                *sym_name_to_idx.get(sym_name).unwrap_or(&0)
            };

            let r_offset = data_vaddr + r.offset;

            // If not RELA, write addend into section data (data_blob)
            if !self.is_rela {
                let offset = r.offset as usize;
                if offset + 8 <= data_blob.len() {
                    let mut cursor = &mut data_blob[offset..];
                    if self.is_64 {
                        cursor.write_u64::<LittleEndian>(r.addend as u64)?;
                    } else {
                        cursor.write_u32::<LittleEndian>(r.addend as u32)?;
                    }
                }
            }

            let r_info = if self.is_64 {
                (sym_idx << 32) | (r_type & 0xffffffff)
            } else {
                ((sym_idx as u64) << 8) | (r_type & 0xff)
            };

            // Collect relocation metadata for testing
            reloc_infos.push(RelocationInfo {
                vaddr: r_offset,
                r_type,
                sym_idx,
            });

            let rel = RelocationEntry {
                offset: r_offset,
                info: r_info,
                addend: r.addend,
                is_rela: self.is_rela,
            };

            let mut ent = vec![];
            self.write_struct(&mut ent, rel)?;

            if self.is_plt_reloc(r_type) {
                rela_plt.extend_from_slice(&ent);
            } else {
                rela_dyn.extend_from_slice(&ent);
            }
        }

        Ok((rela_dyn, rela_plt, relative_count, reloc_infos))
    }
}
