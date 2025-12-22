use anyhow::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use elf::abi::*;

use crate::common::ShdrType;
use crate::dylib::shdr::{Section, SectionAllocator, SectionHeader};
use crate::dylib::{
    RELA_SIZE_32, RELA_SIZE_64, REL_SIZE_32, REL_SIZE_64, SYM_SIZE_32, SYM_SIZE_64,
};

#[derive(Debug, Clone)]
struct DynamicEntry {
    tag: i64,
    value: u64,
}

pub(crate) struct DynamicMetadata {
    dyn_entries: Vec<DynamicEntry>,
}

impl DynamicMetadata {
    pub(crate) fn new() -> Self {
        Self {
            dyn_entries: vec![],
        }
    }

    pub(crate) fn create_section(
        &mut self,
        sections: &mut Vec<Section>,
        is_64: bool,
        is_rela: bool,
        allocator: &mut SectionAllocator,
    ) {
        self.init_from_sections(sections, is_64, is_rela);
        let dyn_size = self.size(is_64);
        let dyn_id = allocator.allocate(dyn_size as usize);
        sections.push(Section {
            header: SectionHeader {
                name_off: 0,
                shtype: ShdrType::Dynamic,
                addr: 0,
                offset: 0,
                size: dyn_size,
                addralign: 8,
            },
            data: dyn_id,
        });
    }

    fn init_from_sections(&mut self, sections: &[Section], is_64: bool, is_rela: bool) {
        for sec in sections {
            let vaddr = sec.header.addr;
            let size = sec.header.size;
            match sec.header.shtype {
                ShdrType::DynStr => {
                    self.add_entry(DT_STRTAB as i64, vaddr);
                    self.add_entry(DT_STRSZ as i64, size);
                }
                ShdrType::DynSym => {
                    self.add_entry(DT_SYMTAB as i64, vaddr);
                    self.add_entry(
                        DT_SYMENT as i64,
                        if is_64 { SYM_SIZE_64 } else { SYM_SIZE_32 },
                    );
                }
                ShdrType::Hash => {
                    self.add_entry(DT_HASH as i64, vaddr);
                }
                ShdrType::Got => {
                    self.add_entry(DT_PLTGOT as i64, vaddr);
                }
                ShdrType::RelaDyn => {
                    self.add_entry(DT_RELA as i64, vaddr);
                    self.add_entry(DT_RELASZ as i64, size);
                    self.add_entry(
                        DT_RELAENT as i64,
                        if is_64 { RELA_SIZE_64 } else { RELA_SIZE_32 },
                    );
                    self.add_entry(DT_RELACOUNT as i64, 0);
                }
                ShdrType::RelDyn => {
                    self.add_entry(DT_REL as i64, vaddr);
                    self.add_entry(DT_RELSZ as i64, size);
                    self.add_entry(
                        DT_RELENT as i64,
                        if is_64 { REL_SIZE_64 } else { REL_SIZE_32 },
                    );
                    self.add_entry(DT_RELCOUNT as i64, 0);
                }
                ShdrType::RelaPlt | ShdrType::RelPlt => {
                    self.add_entry(DT_JMPREL as i64, vaddr);
                    self.add_entry(DT_PLTRELSZ as i64, size);
                    self.add_entry(
                        DT_PLTREL as i64,
                        if is_rela {
                            DT_RELA as u64
                        } else {
                            DT_REL as u64
                        },
                    );
                }
                _ => {}
            }
        }
        self.add_entry(DT_NULL as i64, 0);
    }

    pub(crate) fn add_entry(&mut self, tag: i64, value: u64) {
        self.dyn_entries.push(DynamicEntry { tag, value });
    }

    pub(crate) fn update_entry(&mut self, tag: i64, value: u64) {
        if let Some(entry) = self.dyn_entries.iter_mut().find(|e| e.tag == tag) {
            entry.value = value;
        } else {
            // Insert before DT_NULL if it exists
            if let Some(pos) = self
                .dyn_entries
                .iter()
                .position(|e| e.tag == DT_NULL as i64)
            {
                self.dyn_entries.insert(pos, DynamicEntry { tag, value });
            } else {
                self.add_entry(tag, value);
            }
        }
    }

    pub(crate) fn size(&self, is_64: bool) -> u64 {
        let entry_size = if is_64 { 16 } else { 8 };
        (self.dyn_entries.len() as u64) * entry_size
    }

    pub(crate) fn write<W: std::io::Write>(&self, mut writer: W, is_64: bool) -> Result<()> {
        for entry in &self.dyn_entries {
            if is_64 {
                writer.write_i64::<LittleEndian>(entry.tag)?;
                writer.write_u64::<LittleEndian>(entry.value)?;
            } else {
                writer.write_i32::<LittleEndian>(entry.tag as i32)?;
                writer.write_u32::<LittleEndian>(entry.value as u32)?;
            }
        }
        Ok(())
    }
}

impl DynamicMetadata {
    pub(crate) fn write_to_vec(&self, buf: &mut Vec<u8>, is_64: bool) -> Result<()> {
        buf.clear();
        self.write(buf, is_64)
    }
}
