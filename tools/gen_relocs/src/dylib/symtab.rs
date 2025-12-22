use crate::common::{ShdrType, SymbolDesc, SymbolScope, SymbolType};
use crate::dylib::{
    reloc::RelocMetaData,
    shdr::{Section, SectionAllocator, SectionHeader, SectionId},
    StringTable,
};
use crate::{arch, Arch};
use byteorder::{LittleEndian, WriteBytesExt};
use elf::abi::*;
use std::collections::HashMap;

const HELPER_SUFFIX: &str = "@helper";

pub(crate) struct Symbol {
    name_idx: u32,
    info: u8,
    other: u8,
    shndx: u16,
    value: u64,
    size: u64,
}

impl Symbol {
    fn write(&self, buf: &mut Vec<u8>, is_64: bool) -> std::io::Result<()> {
        if is_64 {
            buf.write_u32::<LittleEndian>(self.name_idx)?;
            buf.write_u8(self.info)?;
            buf.write_u8(self.other)?;
            buf.write_u16::<LittleEndian>(self.shndx)?;
            buf.write_u64::<LittleEndian>(self.value)?;
            buf.write_u64::<LittleEndian>(self.size)?;
        } else {
            buf.write_u32::<LittleEndian>(self.name_idx)?;
            buf.write_u32::<LittleEndian>(self.value as u32)?;
            buf.write_u32::<LittleEndian>(self.size as u32)?;
            buf.write_u8(self.info)?;
            buf.write_u8(self.other)?;
            buf.write_u16::<LittleEndian>(self.shndx)?;
        }
        Ok(())
    }
}

pub(crate) struct SymTabMetadata {
    dynstr: StringTable,
    dynsym: Vec<Symbol>,
    dynsym_shdr_types: Vec<ShdrType>,
    sym_index: HashMap<String, usize>,
    helper_index: HashMap<usize, usize>,
    symbols: Vec<SymbolDesc>,
    dynsym_id: Option<SectionId>,
    text_offset: u64,
    data_offset: u64,
    plt_offset: u64,
    plt0_idx: Option<usize>,
    plt_entries: Vec<(usize, u64)>, // (plt_sym_idx, got_slot_idx)
    arch: Arch,
}

impl SymTabMetadata {
    fn add_symbol(&mut self, name: &str, sym: Symbol, shdr_type: ShdrType) -> usize {
        let sym_idx = self.dynsym.len();
        self.dynsym.push(sym);
        self.dynsym_shdr_types.push(shdr_type);
        self.dynstr.add(name);
        self.sym_index.insert(name.to_string(), sym_idx);
        sym_idx
    }

    pub(crate) fn new(arch: Arch) -> Self {
        let mut symtab = Self {
            dynstr: StringTable::new(),
            dynsym: vec![],
            dynsym_shdr_types: vec![],
            sym_index: HashMap::new(),
            helper_index: HashMap::new(),
            symbols: vec![],
            dynsym_id: None,
            text_offset: 0,
            data_offset: 0,
            plt_offset: 0,
            plt0_idx: None,
            plt_entries: vec![],
            arch,
        };
        // Add NULL symbol
        symtab.add_symbol(
            "",
            Symbol {
                name_idx: 0,
                info: 0,
                other: 0,
                shndx: 0,
                value: 0,
                size: 0,
            },
            ShdrType::Null,
        );
        symtab
    }

    pub(crate) fn add_symbols(&mut self, symbols: &[SymbolDesc]) {
        for s in symbols {
            self.add_single_symbol(s.clone());
        }
    }

    fn add_single_symbol(&mut self, s: SymbolDesc) -> usize {
        let name = s.name.clone();
        let name_idx = self.dynstr.cur_idx();

        let info = match s.scope {
            SymbolScope::Global => STB_GLOBAL,
            SymbolScope::Local => STB_LOCAL,
            SymbolScope::Weak => STB_WEAK,
        } << 4
            | match s.sym_type {
                SymbolType::Func => STT_FUNC,
                SymbolType::Object => STT_OBJECT,
            };

        let (shdr_type, value) = if let Some(content) = &s.content {
            let off = match content.shdr_type {
                ShdrType::Text => {
                    let off = self.text_offset;
                    self.text_offset += content.data.len() as u64;
                    off
                }
                ShdrType::Data => {
                    let off = self.data_offset;
                    self.data_offset += content.data.len() as u64;
                    off
                }
                ShdrType::Plt => {
                    let off = self.plt_offset;
                    self.plt_offset += content.data.len() as u64;
                    off
                }
                _ => todo!("Unsupported shdr_type in SymbolDesc content"),
            };
            (content.shdr_type, off)
        } else {
            // Undefined symbols
            let shdr_type = match s.sym_type {
                SymbolType::Func => ShdrType::Text,
                SymbolType::Object => ShdrType::Data,
            };
            (shdr_type, 0)
        };

        let sym = Symbol {
            name_idx,
            info,
            other: 0,
            shndx: 0,
            value,
            size: s.content.as_ref().map(|c| c.data.len() as u64).unwrap_or(0),
        };
        let idx = self.add_symbol(&name, sym, shdr_type);
        self.symbols.push(s);
        idx
    }

    pub(crate) fn get_text_content(&self) -> Vec<u8> {
        let mut content = vec![];
        for s in &self.symbols {
            if let Some(c) = &s.content {
                if matches!(c.shdr_type, ShdrType::Text) {
                    content.extend_from_slice(&c.data);
                }
            }
        }
        content
    }

    pub(crate) fn get_plt_content(&self) -> Vec<u8> {
        let mut content = vec![];
        for s in &self.symbols {
            if let Some(c) = &s.content {
                if matches!(c.shdr_type, ShdrType::Plt) {
                    content.extend_from_slice(&c.data);
                }
            }
        }
        content
    }

    pub(crate) fn get_data_content(&self) -> Vec<u8> {
        let mut content = vec![];
        for s in &self.symbols {
            if let Some(c) = &s.content {
                if matches!(c.shdr_type, ShdrType::Data) {
                    content.extend_from_slice(&c.data);
                }
            }
        }
        content
    }

    pub(crate) fn create_hashtable(&self, hash_table: &mut Vec<u8>) {
        // nbucket
        let nbucket = 1u32;
        hash_table.extend_from_slice(&nbucket.to_le_bytes());
        // nchain
        let nchain = self.dynsym.len() as u32;
        hash_table.extend_from_slice(&nchain.to_le_bytes());
        // buckets
        // bucket[0] points to the first non-null symbol (index 1)
        let first_sym = if self.dynsym.len() > 1 { 1u32 } else { 0u32 };
        hash_table.extend_from_slice(&first_sym.to_le_bytes());

        // chains
        // chain[0] is always 0
        hash_table.extend_from_slice(&0u32.to_le_bytes());
        for i in 1..self.dynsym.len() {
            let next = if i + 1 < self.dynsym.len() {
                (i + 1) as u32
            } else {
                0u32
            };
            hash_table.extend_from_slice(&next.to_le_bytes());
        }
    }

    pub(crate) fn get_sym_idx(&self, name: &str) -> Option<usize> {
        self.sym_index.get(name).cloned()
    }

    pub(crate) fn get_sym_value_by_idx(&self, idx: usize) -> u64 {
        self.dynsym[idx].value
    }

    fn get_sym_name(&self, idx: usize) -> &str {
        let start = self.dynsym[idx].name_idx as usize;
        self.dynstr.get_str(start)
    }

    pub(crate) fn add_plt_symbols(&mut self, reloc: &RelocMetaData) {
        // Add PLT[0]
        let plt0_code = arch::generate_plt0_code(self.arch);
        let plt0_desc = SymbolDesc::plt_func("PLT0", plt0_code);
        self.add_single_symbol(plt0_desc);
        self.plt0_idx = Some(self.dynsym.len() - 1);

        let mut got_idx = reloc.plt_got_idx() as u64;
        let mut reloc_idx = reloc.plt_start_idx() as u32;

        let plt_relocs = reloc.plt_relocs();
        for reloc in plt_relocs {
            let func_name = self.get_sym_name(reloc.sym_idx() as usize).to_string();
            let plt_sym_name = format!("{}@plt", func_name);

            let plt_code =
                arch::generate_plt_entry_code(self.arch, got_idx, reloc_idx, self.plt_offset);
            let plt_desc = SymbolDesc::plt_func(plt_sym_name.clone(), plt_code);
            let plt_idx = self.add_single_symbol(plt_desc);
            self.plt_entries.push((plt_idx, got_idx));

            let test_helper = format!("{}{}", func_name, HELPER_SUFFIX);
            let helper_code = crate::arch::generate_helper_code(self.arch);
            let helper_desc = SymbolDesc::global_func(test_helper, &helper_code);
            let helper_idx = self.add_single_symbol(helper_desc);
            self.helper_index.insert(plt_idx, helper_idx);

            got_idx += 1;
            reloc_idx += 1;
        }

        // Add IFUNC resolver symbol
        let resolver_name = "__ifunc_resolver";
        let resolver_code = crate::arch::get_ifunc_resolver_code(self.arch);
        let resolver_desc = SymbolDesc::global_func(resolver_name, &resolver_code);
        self.add_single_symbol(resolver_desc);
    }

    pub(crate) fn update_symbol_values(
        &mut self,
        plt_vaddr: u64,
        text_vaddr: u64,
        data_vaddr: u64,
        shdr_map: &HashMap<ShdrType, usize>,
    ) {
        for (i, sym) in self.dynsym.iter_mut().enumerate().skip(1) {
            let shdr_type = self.dynsym_shdr_types[i];
            if let Some(&sec_idx) = shdr_map.get(&shdr_type) {
                sym.shndx = sec_idx as u16;
                let base_vaddr = match shdr_type {
                    ShdrType::Text => text_vaddr,
                    ShdrType::Data => data_vaddr,
                    ShdrType::Plt => plt_vaddr,
                    _ => 0,
                };
                sym.value += base_vaddr;
            }
        }
    }

    pub(crate) fn refill_plt(&self, plt_data: &mut [u8], plt_vaddr: u64, got_vaddr: u64) {
        // 1. Refill PLT0
        if let Some(plt0_idx) = self.plt0_idx {
            let plt0_sym = &self.dynsym[plt0_idx];
            let plt0_off = (plt0_sym.value - plt_vaddr) as usize;
            crate::arch::refill_plt0(self.arch, plt_data, plt0_off, plt0_sym.value, got_vaddr);
        }

        // 2. Refill PLT entries
        for (plt_idx, got_idx) in &self.plt_entries {
            let plt_sym = &self.dynsym[*plt_idx];
            let plt_off = (plt_sym.value - plt_vaddr) as usize;
            let target_got_vaddr = got_vaddr + (got_idx * 8);
            crate::arch::refill_plt_entry(
                self.arch,
                plt_data,
                plt_off,
                plt_sym.value,
                target_got_vaddr,
            );
        }
    }

    pub(crate) fn refill_helpers(&self, text_data: &mut [u8], text_vaddr: u64) {
        for (&plt_idx, &helper_idx) in &self.helper_index {
            let plt_sym = &self.dynsym[plt_idx];
            let helper_sym = &self.dynsym[helper_idx];
            let helper_text_off = (helper_sym.value - text_vaddr) as usize;
            let target_plt_vaddr = plt_sym.value;

            crate::arch::refill_helper(
                self.arch,
                text_data,
                helper_text_off,
                helper_sym.value,
                target_plt_vaddr,
            );
        }
    }

    pub(crate) fn refill_ifunc_resolver(
        &self,
        text_data: &mut [u8],
        text_vaddr: u64,
        plt_vaddr: u64,
    ) {
        let resolver_name = "__ifunc_resolver";
        if let Some(&resolver_idx) = self.sym_index.get(resolver_name) {
            let resolver_sym = &self.dynsym[resolver_idx];
            let resolver_text_off = (resolver_sym.value - text_vaddr) as usize;
            crate::arch::refill_ifunc_resolver(self.arch, text_data, resolver_text_off, plt_vaddr);
        }
    }

    pub(crate) fn refill_dynsym(&self, allocator: &mut SectionAllocator, is_64: bool) {
        if let Some(id) = &self.dynsym_id {
            let buf = allocator.get_mut(id);
            self.write_syms(buf, is_64);
        }
    }

    pub(crate) fn write_syms(&self, buf: &mut Vec<u8>, is_64: bool) {
        buf.clear();
        for sym in &self.dynsym {
            sym.write(buf, is_64).unwrap();
        }
    }

    pub(crate) fn create_sections(
        &mut self,
        is_64: bool,
        allocator: &mut SectionAllocator,
        sections: &mut Vec<Section>,
    ) {
        // Create .dynstr section
        let dynstr_id = allocator.allocate(self.dynstr.data.len());
        let dynstr = allocator.get_mut(&dynstr_id);
        dynstr.copy_from_slice(&self.dynstr.data);
        let dynstr_size = dynstr.len();

        // Create .dynsym section
        let dynsym_id = allocator.allocate(0);
        self.dynsym_id = Some(dynsym_id.clone());
        let dynsym = allocator.get_mut(&dynsym_id);
        for sym in &self.dynsym {
            sym.write(dynsym, is_64).unwrap();
        }
        let dynsym_size = dynsym.len();

        // Create .hash section
        let hash_id = allocator.allocate(0);
        let hash_section = allocator.get_mut(&hash_id);
        self.create_hashtable(hash_section);
        let hash_size = hash_section.len();

        sections.push(Section {
            header: SectionHeader {
                name_off: 0,
                shtype: ShdrType::DynStr,
                addr: 0,
                offset: 0,
                size: dynstr_size as u64,
                addralign: 1,
            },
            data: dynstr_id,
        });
        sections.push(Section {
            header: SectionHeader {
                name_off: 0,
                shtype: ShdrType::DynSym,
                addr: 0,
                offset: 0,
                size: dynsym_size as u64,
                addralign: 8,
            },
            data: dynsym_id,
        });
        sections.push(Section {
            header: SectionHeader {
                name_off: 0,
                shtype: ShdrType::Hash,
                addr: 0,
                offset: 0,
                size: hash_size as u64,
                addralign: 4,
            },
            data: hash_id,
        });
    }
}
