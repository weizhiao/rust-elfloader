#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SymbolType {
    Func,
    Object,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SymbolScope {
    Global,
    Local,
    Weak,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum ShdrType {
    Null,
    DynStr,
    DynSym,
    RelaDyn,
    RelaPlt,
    RelDyn,
    RelPlt,
    Dynamic,
    Hash,
    ShStrTab,
    Plt,
    Text,
    Data,
    Got,
}

#[derive(Debug, Clone)]
pub struct Content {
    pub data: Vec<u8>,
    pub shdr_type: ShdrType,
}

#[derive(Clone, Debug)]
pub struct SymbolDesc {
    pub name: String,
    pub sym_type: SymbolType,
    pub scope: SymbolScope,
    pub content: Option<Content>,
}

impl SymbolDesc {
    /// Create a global function symbol with associated code.
    pub fn global_func(name: impl Into<String>, code: &[u8]) -> Self {
        Self {
            name: name.into(),
            sym_type: SymbolType::Func,
            scope: SymbolScope::Global,
            content: Some(Content {
                data: code.to_vec(),
                shdr_type: ShdrType::Text,
            }),
        }
    }

    /// Create a global data object symbol.
    pub fn global_object(name: impl Into<String>, data: &[u8]) -> Self {
        Self {
            name: name.into(),
            sym_type: SymbolType::Object,
            scope: SymbolScope::Global,
            content: Some(Content {
                data: data.to_vec(),
                shdr_type: ShdrType::Data,
            }),
        }
    }

    pub fn undefined_func(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sym_type: SymbolType::Func,
            scope: SymbolScope::Global,
            content: None,
        }
    }

    pub fn undefined_object(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sym_type: SymbolType::Object,
            scope: SymbolScope::Global,
            content: None,
        }
    }

    pub fn plt_func(name: impl Into<String>, code: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            sym_type: SymbolType::Func,
            scope: SymbolScope::Global,
            content: Some(Content {
                data: code,
                shdr_type: ShdrType::Plt,
            }),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RelocType(pub u32);

impl RelocType {
    pub fn as_u32(&self) -> u32 {
        self.0
    }

    pub fn as_u64(&self) -> u64 {
        self.0 as u64
    }
}

/// Represents a relocation entry with flexible symbol reference
pub struct RelocEntry {
    /// Symbol name that this relocation references
    pub symbol_name: String,
    pub r_type: RelocType,
}

impl RelocEntry {
    /// Create a new relocation entry
    pub fn new(symbol_name: impl Into<String>, r_type: u32) -> Self {
        Self {
            symbol_name: symbol_name.into(),
            r_type: RelocType(r_type),
        }
    }
}
