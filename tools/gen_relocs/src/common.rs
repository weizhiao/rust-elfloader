use object::write::{RelocationFlags, SectionId, SymbolId};

/// Represents a relocation entry with flexible symbol reference
/// Both offset and addend are automatically calculated by gen_relocs based on relocation type
pub struct RelocEntry {
    /// Symbol name that this relocation references
    pub symbol_name: String,
    pub flags: RelocationFlags,
}

impl RelocEntry {
    /// Create a new relocation entry
    /// Offset and addend will be auto-calculated based on relocation type and sequence
    pub fn new(symbol_name: impl Into<String>, flags: RelocationFlags) -> Self {
        Self {
            symbol_name: symbol_name.into(),
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
