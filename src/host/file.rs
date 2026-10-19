//! Read export metadata through PT_DYNAMIC and file-backed PT_LOAD ranges.
use super::error::metadata;
use crate::{
    ByteRepr, Loader, Result,
    elf::{
        ElfDyn, ElfDynamicTag as Tag, ElfGnuHeader, ElfPhdr, ElfProgramType, ElfSymbol,
        ElfSymbolBind, ElfSymbolType, ElfSymbolVisibility, parse_dynamic_entries,
        version::{VerDef, VerDefAux},
    },
    input::{ElfReader, ElfReaderExt},
    tls::TlsInfo,
};
use alloc::{collections::BTreeMap, ffi::CString, vec::Vec};
use core::mem::size_of;

pub(super) struct Export {
    pub symbol: ElfSymbol,
    pub name: CString,
    pub version: Option<(CString, bool)>,
}

pub(super) struct FileExports {
    pub tls: Option<TlsInfo>,
    pub symbols: Vec<Export>,
}

impl FileExports {
    pub(super) fn read(file: &impl ElfReader) -> Result<Self> {
        let header = Loader::new().read_ehdr(file)?;
        let (offset, _) = header
            .checked_phdr_layout(file.len())?
            .ok_or_else(|| metadata("missing program headers"))?;
        let headers = file.read_to_vec::<ElfPhdr>(offset, header.e_phnum())?;
        let reader = FileView {
            file,
            headers: &headers,
        };
        let dynamic = headers
            .iter()
            .find(|p| p.program_type() == ElfProgramType::DYNAMIC)
            .ok_or_else(|| metadata("missing PT_DYNAMIC"))?;
        if dynamic.p_filesz() % size_of::<ElfDyn>() != 0 {
            return Err(metadata("invalid PT_DYNAMIC size"));
        }
        reader.check_file(dynamic.p_offset(), dynamic.p_filesz())?;
        let entries = file
            .read_to_vec::<ElfDyn>(dynamic.p_offset(), dynamic.p_filesz() / size_of::<ElfDyn>())?;
        let end = entries
            .iter()
            .position(|entry| entry.tag() == Tag::NULL)
            .ok_or_else(|| metadata("unterminated PT_DYNAMIC"))?;
        let entries = &entries[..end];
        let value = |tag| {
            entries
                .iter()
                .find(|entry| entry.tag() == tag)
                .map(|entry| entry.value())
        };
        if value(Tag::SYMENT) != Some(size_of::<ElfSymbol>()) {
            return Err(metadata("invalid DT_SYMENT"));
        }
        if value(Tag::SYMTAB).is_none() || value(Tag::STRTAB).is_none() {
            return Err(metadata("missing dynamic symbol or string table"));
        }
        let dynamic =
            parse_dynamic_entries(entries.iter().map(|entry| (entry.tag(), entry.value())));
        let strings = reader.read::<u8>(
            dynamic.strtab_off,
            dynamic
                .strtab_size
                .ok_or_else(|| metadata("missing DT_STRSZ"))?
                .get(),
        )?;
        let count = if let Some(address) = dynamic.elf_hash_off {
            let header = reader.read::<u32>(address, 2)?;
            let words = add(2, add(header[0] as usize, header[1] as usize)?)?;
            reader.range(address, mul(words, size_of::<u32>())?)?;
            header[1] as usize
        } else if let Some(address) = dynamic.gnu_hash_off {
            reader.gnu_symbol_count(address)?
        } else {
            return Err(metadata("missing dynamic symbol hash table"));
        };
        let symbols = reader.read::<ElfSymbol>(dynamic.symtab_off, count)?;
        let versions = dynamic
            .version_ids_off
            .map(|address| reader.read::<u16>(address.get(), count))
            .transpose()?;
        let mut definitions = BTreeMap::new();
        if let Some(address) = dynamic.verdef_off {
            let count = value(Tag::VERDEFNUM).ok_or_else(|| metadata("missing DT_VERDEFNUM"))?;
            let mut address = address.get();
            for index in 0..count {
                let definition = reader.record::<VerDef>(address)?;
                if definition.vd_version != 1
                    || definition.vd_cnt == 0
                    || definition.vd_aux < size_of::<VerDef>() as u32
                {
                    return Err(metadata("invalid version definition"));
                }
                let aux = reader.record::<VerDefAux>(add(address, definition.vd_aux as usize)?)?;
                if definition.vd_flags & 1 == 0 {
                    definitions
                        .insert(definition.index(), string(&strings, aux.vda_name as usize)?);
                }
                if index + 1 < count {
                    if definition.vd_next < size_of::<VerDef>() as u32 {
                        return Err(metadata("truncated version definitions"));
                    }
                    address = add(address, definition.vd_next as usize)?;
                } else if definition.vd_next != 0 {
                    return Err(metadata("incorrect DT_VERDEFNUM"));
                }
            }
        }
        let mut exports = Vec::new();
        for (index, symbol) in symbols.into_iter().enumerate().skip(1) {
            if !is_export(&symbol) {
                continue;
            }
            let version = if let Some(ids) = &versions {
                let id = ids[index];
                let index = (id & 0x7fff) as usize;
                if index > 1 {
                    Some((
                        definitions
                            .get(&index)
                            .ok_or_else(|| metadata("missing export version definition"))?
                            .clone(),
                        id & 0x8000 == 0,
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            exports.push(Export {
                name: string(&strings, symbol.st_name())?,
                symbol,
                version,
            });
        }
        Ok(Self {
            tls: headers
                .iter()
                .find(|p| p.program_type() == ElfProgramType::TLS)
                .map(TlsInfo::new),
            symbols: exports,
        })
    }
}

struct FileView<'a, R> {
    file: &'a R,
    headers: &'a [ElfPhdr],
}

impl<R: ElfReader> FileView<'_, R> {
    fn check_file(&self, offset: usize, size: usize) -> Result<()> {
        if add(offset, size)? > self.file.len() {
            return Err(metadata("metadata outside ELF file"));
        }
        Ok(())
    }

    fn range(&self, address: usize, size: usize) -> Result<usize> {
        for header in self.headers {
            if header.program_type() != ElfProgramType::LOAD {
                continue;
            }
            let Some(delta) = address.checked_sub(header.p_vaddr().get()) else {
                continue;
            };
            if delta <= header.p_filesz() && size <= header.p_filesz() - delta {
                let offset = add(header.p_offset(), delta)?;
                self.check_file(offset, size)?;
                return Ok(offset);
            }
        }
        Err(metadata("metadata outside file-backed PT_LOAD ranges"))
    }

    fn read<T: ByteRepr>(&self, address: usize, count: usize) -> Result<Vec<T>> {
        self.file
            .read_to_vec(self.range(address, mul(count, size_of::<T>())?)?, count)
    }

    fn record<T: ByteRepr + Copy>(&self, address: usize) -> Result<T> {
        Ok(self.read::<T>(address, 1)?[0])
    }

    fn gnu_symbol_count(&self, address: usize) -> Result<usize> {
        let header = self.record::<ElfGnuHeader>(address)?;
        if header.nbucket == 0 || header.nbloom == 0 || !header.nbloom.is_power_of_two() {
            return Err(metadata("invalid GNU hash header"));
        }
        let buckets_address = add(
            add(address, size_of::<ElfGnuHeader>())?,
            mul(header.nbloom as usize, size_of::<usize>())?,
        )?;
        let buckets = self.read::<u32>(buckets_address, header.nbucket as usize)?;
        let bias = header.symbias as usize;
        if buckets
            .iter()
            .any(|&bucket| bucket != 0 && (bucket as usize) < bias)
        {
            return Err(metadata("invalid GNU hash bucket"));
        }
        // GNU hash chains follow symbol order, so the last bucket ends at the final symbol.
        let last = buckets.into_iter().max().unwrap_or(0) as usize;
        if last == 0 {
            return Ok(bias);
        }
        let chains = add(
            buckets_address,
            mul(header.nbucket as usize, size_of::<u32>())?,
        )?;
        let mut index = last;
        loop {
            let hash = self.record::<u32>(add(chains, mul(index - bias, size_of::<u32>())?)?)?;
            index = add(index, 1)?;
            if hash & 1 != 0 {
                return Ok(index);
            }
        }
    }
}

fn add(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right)
        .ok_or_else(|| metadata("ELF metadata address overflow"))
}

fn mul(left: usize, right: usize) -> Result<usize> {
    left.checked_mul(right)
        .ok_or_else(|| metadata("ELF metadata size overflow"))
}

fn string(strings: &[u8], offset: usize) -> Result<CString> {
    let tail = strings
        .get(offset..)
        .ok_or_else(|| metadata("string offset outside DT_STRTAB"))?;
    let end = tail
        .iter()
        .position(|&byte| byte == 0)
        .ok_or_else(|| metadata("unterminated ELF string"))?;
    CString::new(&tail[..end]).map_err(metadata)
}

fn is_export(symbol: &ElfSymbol) -> bool {
    matches!(
        symbol.visibility(),
        ElfSymbolVisibility::DEFAULT | ElfSymbolVisibility::PROTECTED
    ) && matches!(symbol.bind(), ElfSymbolBind::GLOBAL | ElfSymbolBind::WEAK)
        && matches!(
            symbol.symbol_type(),
            ElfSymbolType::FUNC
                | ElfSymbolType::GNU_IFUNC
                | ElfSymbolType::OBJECT
                | ElfSymbolType::TLS
        )
        && !symbol.is_undef()
}
