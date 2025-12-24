use crate::common::SectionKind;
use crate::dylib::shdr::{Section, SectionAllocator, SectionHeader, SectionId};
use crate::dylib::symtab::SymTabMetadata;

pub(crate) struct TlsMetaData {
    tls_id: Option<SectionId>,
    size: u64,
}

impl TlsMetaData {
    pub(crate) fn new(symtab: &SymTabMetadata, allocator: &mut SectionAllocator) -> Self {
        let tls_content = symtab.get_tls_content();
        let size = tls_content.len() as u64;
        let tls_id = if !tls_content.is_empty() {
            Some(allocator.allocate_with_data(tls_content))
        } else {
            None
        };
        Self { tls_id, size }
    }

    pub(crate) fn create_section(&self, sections: &mut Vec<Section>) {
        if let Some(tls_id) = self.tls_id {
            sections.push(Section {
                header: SectionHeader {
                    name_off: 0,
                    shtype: SectionKind::Tls,
                    addr: 0,
                    offset: 0,
                    size: self.size,
                    addralign: 8,
                },
                data: tls_id,
            });
        }
    }
}
