use crate::common::ShdrType;
use crate::dylib::{
    reloc::RelocMetaData,
    shdr::{Section, SectionAllocator, SectionHeader, SectionId},
    symtab::SymTabMetadata,
};

pub(crate) struct DataMetaData {
    data_id: SectionId,
    got_id: SectionId,
}

impl DataMetaData {
    pub(crate) fn new(
        reloc: &RelocMetaData,
        symtab: &SymTabMetadata,
        is_64: bool,
        allocator: &mut SectionAllocator,
    ) -> Self {
        let got_size = reloc.got_slot_count() * if is_64 { 8 } else { 4 };
        let data_content = symtab.get_data_content();
        let data_id = allocator.allocate_with_data(data_content);
        let got_id = allocator.allocate(got_size);
        Self { data_id, got_id }
    }

    pub(crate) fn create_sections(
        &self,
        allocator: &SectionAllocator,
        sections: &mut Vec<Section>,
    ) {
        let data_size = allocator.get(&self.data_id).len();
        let got_size = allocator.get(&self.got_id).len();
        let data = SectionHeader {
            name_off: 0,
            shtype: ShdrType::Data,
            addr: 0,
            offset: 0,
            size: data_size as u64,
            addralign: 8,
        };
        let got = SectionHeader {
            name_off: 0,
            shtype: ShdrType::Got,
            addr: 0,
            offset: 0,
            size: got_size as u64,
            addralign: 8,
        };
        sections.push(Section {
            header: data,
            data: self.data_id.clone(),
        });
        sections.push(Section {
            header: got,
            data: self.got_id.clone(),
        });
    }
}
