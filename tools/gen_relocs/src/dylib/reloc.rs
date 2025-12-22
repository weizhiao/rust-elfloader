use crate::common::{RelocEntry, RelocType, ShdrType};
use crate::{
    dylib::{
        shdr::{Section, SectionAllocator, SectionHeader, SectionId},
        symtab::SymTabMetadata,
        RelocationInfo,
    },
    Arch,
};
use anyhow::Result;
use byteorder::{LittleEndian, WriteBytesExt};

pub(crate) struct RelocMetaData {
    arch: Arch,
    relocs: Vec<Reloc>,
    relative_count: usize,
    got_count: usize,
    plt_count: usize,
    pub(crate) rel_dyn_id: SectionId,
    pub(crate) rel_plt_id: SectionId,
}

impl RelocMetaData {
    pub(crate) fn new(
        arch: Arch,
        raw: &[RelocEntry],
        symbols: &SymTabMetadata,
        allocator: &mut SectionAllocator,
    ) -> Result<Self> {
        let mut relocs = Vec::new();

        // Process relocations in two passes: RELATIVE first, then others
        let mut relative_relocs = Vec::new();
        let mut got_relocs = Vec::new();
        let mut plt_relocs = Vec::new();

        for (idx, r) in raw.iter().enumerate() {
            if r.r_type.is_relative_reloc(arch) {
                relative_relocs.push((idx, r));
            } else if r.r_type.is_plt_reloc(arch) || r.r_type.is_irelative_reloc(arch) {
                plt_relocs.push((idx, r));
            } else {
                debug_assert!(r.r_type.is_got_reloc(arch));
                got_relocs.push((idx, r));
            }
        }

        let relative_count = relative_relocs.len();
        let got_count = got_relocs.len();
        let plt_count = plt_relocs.len();

        // Process all relocations
        for (idx, r) in relative_relocs
            .into_iter()
            .chain(got_relocs.into_iter())
            .chain(plt_relocs.into_iter())
        {
            let sym_idx = if r.r_type.is_relative_reloc(arch) {
                0
            } else {
                symbols.get_sym_idx(&r.symbol_name).unwrap() as u64
            };

            let reloc_offset = (3 + (idx as u64)) * if arch.is_64() { 8 } else { 4 };

            relocs.push(Reloc {
                sym_idx,
                offset: reloc_offset,
                addend: 0, // Will be calculated in fill_sections
                r_type: r.r_type,
            });
        }

        let is_64 = arch.is_64();
        let is_rela = arch.is_rela();
        let entry_size = if is_64 {
            if is_rela {
                24
            } else {
                16
            }
        } else {
            if is_rela {
                12
            } else {
                8
            }
        };

        let rel_dyn_id = allocator.allocate((relative_count + got_count) * entry_size);
        let rel_plt_id = allocator.allocate(plt_count * entry_size);

        Ok(Self {
            arch,
            relocs,
            relative_count,
            got_count,
            plt_count,
            rel_dyn_id,
            rel_plt_id,
        })
    }

    pub(crate) fn got_slot_count(&self) -> usize {
        self.got_count + self.plt_count
    }

    pub(crate) fn get_relocation_infos(&self, got_vaddr: u64) -> Vec<RelocationInfo> {
        self.relocs
            .iter()
            .map(|r| RelocationInfo {
                vaddr: got_vaddr + r.offset,
                r_type: r.r_type.0,
                sym_idx: r.sym_idx,
                addend: r.addend,
            })
            .collect()
    }

    pub(crate) fn plt_relocs(&self) -> &[Reloc] {
        let start = self.relative_count + self.got_count;
        &self.relocs[start..]
    }

    pub(crate) fn plt_got_idx(&self) -> usize {
        self.got_count
    }

    pub(crate) fn relative_count(&self) -> usize {
        self.relative_count
    }

    pub(crate) fn plt_start_idx(&self) -> usize {
        self.relative_count + self.got_count
    }

    pub(crate) fn fill_reloc(
        &mut self,
        is_rela: bool,
        is_64: bool,
        got_id: &SectionId,
        got_vaddr: u64,
        data_vaddr: u64,
        symbols: &SymTabMetadata,
        allocator: &mut SectionAllocator,
    ) -> Result<()> {
        allocator.get_mut(&self.rel_dyn_id).clear();
        allocator.get_mut(&self.rel_plt_id).clear();

        let plt_start = self.relative_count + self.got_count;
        for reloc in self.relocs.iter_mut().take(plt_start) {
            let sym_value = if reloc.sym_idx != 0 {
                symbols.get_sym_value_by_idx(reloc.sym_idx as usize)
            } else {
                0
            };
            reloc.addend = crate::arch::calculate_addend(
                self.arch,
                reloc.r_type,
                reloc.offset,
                data_vaddr,
                sym_value,
            );
            if !is_rela {
                let offset = reloc.offset as usize;
                let got = allocator.get_mut(got_id);
                let mut cursor = &mut got[offset..];
                if is_64 {
                    cursor.write_u64::<LittleEndian>(reloc.addend as u64)?;
                } else {
                    cursor.write_u32::<LittleEndian>(reloc.addend as u32)?;
                }
            }
            let final_offset = reloc.offset + got_vaddr;
            let mut temp_reloc = reloc.clone();
            temp_reloc.offset = final_offset;
            temp_reloc.write(
                allocator.get_mut(&self.rel_dyn_id),
                self.arch,
                is_64,
                is_rela,
            )?;
        }

        for reloc in self.relocs.iter_mut().skip(plt_start) {
            let sym_value = if reloc.sym_idx != 0 {
                symbols.get_sym_value_by_idx(reloc.sym_idx as usize)
            } else {
                0
            };
            reloc.addend = crate::arch::calculate_addend(
                self.arch,
                reloc.r_type,
                reloc.offset,
                data_vaddr,
                sym_value,
            );
            if !is_rela {
                let offset = reloc.offset as usize;
                let got = allocator.get_mut(got_id);
                let mut cursor = &mut got[offset..];
                if is_64 {
                    cursor.write_u64::<LittleEndian>(reloc.addend as u64)?;
                } else {
                    cursor.write_u32::<LittleEndian>(reloc.addend as u32)?;
                }
            }
            let final_offset = reloc.offset + got_vaddr;
            let mut temp_reloc = reloc.clone();
            temp_reloc.offset = final_offset;
            temp_reloc.write(
                allocator.get_mut(&self.rel_plt_id),
                self.arch,
                is_64,
                is_rela,
            )?;
        }
        Ok(())
    }

    pub(crate) fn create_sections(
        &mut self,
        is_rela: bool,
        is_64: bool,
        allocator: &mut SectionAllocator,
        sections: &mut Vec<Section>,
    ) -> Result<()> {
        let rel_dyn_size = allocator.get(&self.rel_dyn_id).len() as u64;
        let rel_plt_size = allocator.get(&self.rel_plt_id).len() as u64;

        sections.push(Section {
            header: SectionHeader {
                name_off: 0,
                shtype: if is_rela {
                    ShdrType::RelaDyn
                } else {
                    ShdrType::RelDyn
                },
                addr: 0,
                offset: 0,
                size: rel_dyn_size,
                addralign: if is_64 { 8 } else { 4 },
            },
            data: self.rel_dyn_id,
        });
        sections.push(Section {
            header: SectionHeader {
                name_off: 0,
                shtype: if is_rela {
                    ShdrType::RelaPlt
                } else {
                    ShdrType::RelPlt
                },
                addr: 0,
                offset: 0,
                size: rel_plt_size,
                addralign: if is_64 { 8 } else { 4 },
            },
            data: self.rel_plt_id,
        });
        Ok(())
    }
}

/// Pending relocation entry before virtual addresses are determined
#[derive(Clone, Debug)]
pub(crate) struct Reloc {
    /// Symbol index in symbol table
    sym_idx: u64,
    /// Offset within data segment
    offset: u64,
    /// Addend value
    addend: i64,
    /// Relocation type
    r_type: RelocType,
}

impl Reloc {
    pub(crate) fn sym_idx(&self) -> u64 {
        self.sym_idx
    }

    fn write(&self, buf: &mut Vec<u8>, arch: Arch, is_64: bool, is_rela: bool) -> Result<()> {
        let sym_idx = if self.r_type.is_irelative_reloc(arch) || self.r_type.is_relative_reloc(arch)
        {
            0
        } else {
            self.sym_idx
        };

        if is_64 {
            buf.write_u64::<LittleEndian>(self.offset)?;
            let r_info = (sym_idx << 32) | (self.r_type.as_u64() & 0xffffffff);
            buf.write_u64::<LittleEndian>(r_info)?;
            if is_rela {
                buf.write_i64::<LittleEndian>(self.addend)?;
            }
        } else {
            buf.write_u32::<LittleEndian>(self.offset as u32)?;
            let r_info = ((sym_idx as u32) << 8) | (self.r_type.as_u32() & 0xff);
            buf.write_u32::<LittleEndian>(r_info)?;
            if is_rela {
                buf.write_i32::<LittleEndian>(self.addend as i32)?;
            }
        }
        Ok(())
    }
}
