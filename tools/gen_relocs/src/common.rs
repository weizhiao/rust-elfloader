#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SymbolType {
    Func,
    Object,
    Tls,
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
    GotPlt,
    Tls,
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
    pub size: Option<u64>,
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
            size: None,
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
            size: None,
        }
    }

    pub fn undefined_func(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sym_type: SymbolType::Func,
            scope: SymbolScope::Global,
            content: None,
            size: None,
        }
    }

    pub fn undefined_object(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sym_type: SymbolType::Object,
            scope: SymbolScope::Global,
            content: None,
            size: None,
        }
    }

    pub fn global_tls(name: impl Into<String>, data: &[u8]) -> Self {
        Self {
            name: name.into(),
            sym_type: SymbolType::Tls,
            scope: SymbolScope::Global,
            content: Some(Content {
                data: data.to_vec(),
                shdr_type: ShdrType::Tls,
            }),
            size: None,
        }
    }

    pub fn undefined_tls(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sym_type: SymbolType::Tls,
            scope: SymbolScope::Global,
            content: None,
            size: None,
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
            size: None,
        }
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
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
    pub addend: i64,
}

impl RelocEntry {
    /// Create a new relocation entry
    pub fn with_name(symbol_name: impl Into<String>, r_type: u32) -> Self {
        Self {
            symbol_name: symbol_name.into(),
            r_type: RelocType(r_type),
            addend: 0,
        }
    }

    /// Create a new relocation entry without a symbol name
    pub fn new(r_type: u32) -> Self {
        Self {
            symbol_name: String::new(),
            r_type: RelocType(r_type),
            addend: 0,
        }
    }

    /// Set addend for the relocation
    pub fn with_addend(mut self, addend: i64) -> Self {
        self.addend = addend;
        self
    }
}
