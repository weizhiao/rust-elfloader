use crate::common::ShdrType;
use crate::dylib::{
    shdr::{Section, SectionAllocator, SectionHeader, SectionId},
    symtab::SymTabMetadata,
};

pub(crate) struct CodeMetaData {
    text_id: SectionId,
    plt_id: SectionId,
}

impl CodeMetaData {
    pub(crate) fn new(symtab: &SymTabMetadata, allocator: &mut SectionAllocator) -> Self {
        let text_data = symtab.get_text_content();
        let plt_data = symtab.get_plt_content();
        let text_id = allocator.allocate_with_data(text_data);
        let plt_id = allocator.allocate_with_data(plt_data);
        Self { text_id, plt_id }
    }

    pub(crate) fn create_sections(
        &self,
        allocator: &SectionAllocator,
        sections: &mut Vec<Section>,
    ) {
        let text_size = allocator.get(&self.text_id).len();
        let plt_size = allocator.get(&self.plt_id).len();

        sections.push(Section {
            header: SectionHeader {
                name_off: 0,
                shtype: ShdrType::Text,
                addr: 0,
                offset: 0,
                size: text_size as u64,
                addralign: 16,
            },
            data: self.text_id.clone(),
        });
        sections.push(Section {
            header: SectionHeader {
                name_off: 0,
                shtype: ShdrType::Plt,
                addr: 0,
                offset: 0,
                size: plt_size as u64,
                addralign: 16,
            },
            data: self.plt_id.clone(),
        });
    }

    pub(crate) fn refill_text(
        &mut self,
        plt_vaddr: u64,
        text_vaddr: u64,
        got_vaddr: u64,
        symtab: &SymTabMetadata,
        allocator: &mut SectionAllocator,
    ) {
        let text_data = allocator.get_mut(&self.text_id);
        // Update IFUNC resolver
        symtab.refill_ifunc_resolver(text_data, text_vaddr, plt_vaddr);

        // Update helpers
        symtab.refill_helpers(text_data, text_vaddr);

        // Update PLT entries
        let plt_data = allocator.get_mut(&self.plt_id);
        symtab.refill_plt(plt_data, plt_vaddr, got_vaddr);
    }
}
