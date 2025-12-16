use object::write::{RelocationFlags, SectionId, SymbolId};

/// Represents a relocation entry with flexible symbol reference
pub struct RelocEntry {
    pub offset: u64,
    /// Symbol name that this relocation references
    pub symbol_name: String,
    pub addend: i64,
    pub flags: RelocationFlags,
}

impl RelocEntry {
    /// Create a new relocation entry
    pub fn new(
        offset: u64,
        symbol_name: impl Into<String>,
        addend: i64,
        flags: RelocationFlags,
    ) -> Self {
        Self {
            offset,
            symbol_name: symbol_name.into(),
            addend,
            flags,
        }
    }
}

pub struct RelocFixtures {
    pub data_id: SectionId,
    #[allow(dead_code)]
    pub external_func: SymbolId,
    #[allow(dead_code)]
    pub external_var: SymbolId,
    #[allow(dead_code)]
    pub external_tls: SymbolId,
    #[allow(dead_code)]
    pub data_section_sym: SymbolId,
}
