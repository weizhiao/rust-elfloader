//! Explicit snapshots of the glibc ldconfig cache.
//!
//! Format definitions: glibc sysdeps/generic/dl-cache.h. Parsing uses checked
//! byte reads rather than native C struct casts, including for unaligned input.

use crate::{
    IoError, ParseLdCacheError, Result,
    input::{ElfFile, ElfReader, ElfReaderExt, Path, PathBuf},
    sync::Arc,
};
use alloc::{boxed::Box, vec::Vec};
use core::{ffi::CStr, fmt, ops::Range};

const OLD_MAGIC: &[u8] = b"ld.so-1.7.0";
const NEW_MAGIC: &[u8] = b"glibc-ld.so.cache";
const NEW_VERSION: &[u8] = b"1.1";
const OLD_HEADER_SIZE: usize = 16;
const NEW_HEADER_SIZE: usize = 48;
const OLD_ENTRY_SIZE: usize = 12;
const NEW_ENTRY_SIZE: usize = 24;

struct CacheEntry {
    name: Box<[u8]>,
    path: PathBuf,
}

/// An immutable, shareable snapshot of a glibc `ld.so.cache` file.
///
/// Supports the legacy, version 1.1, and combined compatibility formats. Names
/// and paths are byte strings. Cache entries for generic ELF/glibc libraries
/// are indexed by SONAME, retaining the file's order among equal names.
///
/// This is a cache source, not a complete glibc search policy. Hardware-specific
/// entries (legacy HWCAP and `glibc-hwcaps`) are excluded: ELF machine/class checks
/// alone cannot establish that their instructions are safe for the host CPU.
/// Generic entries remain available. Environment variables, default directories,
/// `DF_1_NODEFLIB`, and glibc tunables are not interpreted here.
///
/// Parsing and file I/O happen only when constructing the snapshot; clones share
/// its index. Replacing the file does not refresh an existing snapshot. Callers
/// choose when to reload it and where to insert it in their resolver's search order.
#[derive(Clone)]
pub struct LdCache {
    entries: Arc<[CacheEntry]>,
}

impl fmt::Debug for LdCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LdCache")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl LdCache {
    /// Reads `/etc/ld.so.cache`. Missing files and invalid caches return errors.
    /// No process-global state or implicit fallback search path is installed.
    pub fn load_system() -> Result<Self> {
        Self::load("/etc/ld.so.cache")
    }

    /// Reads a cache file using the configured platform file backend.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let file = ElfFile::from_path(path)?;
        let bytes = file.read_to_vec::<u8>(0, file.len())?;
        Self::parse(&bytes)
    }

    /// Parses a cache into an owned snapshot without accessing the filesystem.
    /// Old caches without a byte-order marker use the current target's byte order.
    /// Modern caches with a marker can be parsed in either byte order.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let table = CacheTable::parse(bytes)?;
        let mut entries = Vec::new();
        entries
            .try_reserve(table.count)
            .map_err(|_| IoError::OutOfMemory)?;
        for index in 0..table.count {
            // parse() has already checked the complete entry array's bounds.
            let offset = table.entries_start + index * table.entry_size;
            let flags = table.u32(offset)?;
            let name = table.string(table.u32(offset + 4)?)?;
            let path = table.string(table.u32(offset + 8)?)?;
            if name.is_empty() || name.contains(&b'/') || !path.starts_with(b"/") {
                return Err(ParseLdCacheError::InvalidEntry.into());
            }
            // Keep generic ELF (1) and glibc (3), excluding libc4/libc5 entries.
            // The resolver validates the actual ELF header for its chosen Arch.
            if !matches!(flags & 0xff, 1 | 3) {
                continue;
            }
            if table.entry_size == NEW_ENTRY_SIZE && table.u64(offset + 16)? != 0 {
                continue;
            }
            let path = Path::from_elf_bytes(path).ok_or(ParseLdCacheError::InvalidPath)?;
            entries.push(CacheEntry {
                name: name.into(),
                path: path.into(),
            });
        }
        // glibc uses a numeric SONAME comparator. An independent bytewise index
        // avoids relying on that ordering and on the input having been sorted.
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Self {
            entries: entries.into(),
        })
    }

    /// Returns generic cached file candidates in cache preference order.
    ///
    /// Entries may describe different ELF architectures or stale files. Callers
    /// must validate candidates before use; `SearchPathResolver::push_ld_cache`
    /// performs the normal target-specific ELF header checks on each candidate.
    pub fn candidates(&self, name: impl AsRef<[u8]>) -> impl Iterator<Item = &Path> {
        let name = name.as_ref();
        let start = self
            .entries
            .partition_point(|entry| entry.name.as_ref() < name);
        let remaining = &self.entries[start..];
        let count = remaining.partition_point(|entry| entry.name.as_ref() == name);
        remaining[..count].iter().map(|entry| entry.path.as_path())
    }
}

struct CacheTable<'a> {
    bytes: &'a [u8],
    little_endian: bool,
    entries_start: usize,
    entry_size: usize,
    count: usize,
    // Modern offsets are relative to the new header; legacy offsets are relative
    // to the end of the legacy entry array. The valid string range is narrower.
    string_offset_base: usize,
    strings: Range<usize>,
}

impl<'a> CacheTable<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.starts_with(NEW_MAGIC) {
            return Self::modern(bytes, 0);
        }
        if !bytes.starts_with(OLD_MAGIC) {
            return Err(ParseLdCacheError::InvalidMagic.into());
        }
        let count = read_u32(bytes, 12, cfg!(target_endian = "little"))? as usize;
        let entries_end = checked_table_end(OLD_HEADER_SIZE, count, OLD_ENTRY_SIZE, bytes.len())?;
        let new_header = entries_end
            .checked_add(7)
            .ok_or(ParseLdCacheError::Truncated)?
            & !7;
        if bytes
            .get(new_header..)
            .is_some_and(|tail| tail.starts_with(NEW_MAGIC))
        {
            return Self::modern(bytes, new_header);
        }
        Ok(Self {
            bytes,
            little_endian: cfg!(target_endian = "little"),
            entries_start: OLD_HEADER_SIZE,
            entry_size: OLD_ENTRY_SIZE,
            count,
            string_offset_base: entries_end,
            strings: entries_end..bytes.len(),
        })
    }

    fn modern(bytes: &'a [u8], header: usize) -> Result<Self> {
        let prefix = read_bytes::<NEW_HEADER_SIZE>(bytes, header)?;
        if &prefix[NEW_MAGIC.len()..20] != NEW_VERSION {
            return Err(ParseLdCacheError::UnsupportedVersion.into());
        }
        let little_endian = match prefix[28] & 3 {
            0 => cfg!(target_endian = "little"),
            2 => true,
            3 => false,
            _ => return Err(ParseLdCacheError::InvalidByteOrder.into()),
        };
        let count = read_u32(bytes, header + 20, little_endian)? as usize;
        let strings_len = read_u32(bytes, header + 24, little_endian)? as usize;
        let entries_start = header
            .checked_add(NEW_HEADER_SIZE)
            .ok_or(ParseLdCacheError::Truncated)?;
        let entries_end = checked_table_end(entries_start, count, NEW_ENTRY_SIZE, bytes.len())?;
        let strings_end = entries_end
            .checked_add(strings_len)
            .filter(|end| *end <= bytes.len())
            .ok_or(ParseLdCacheError::Truncated)?;
        // Extensions only govern specialized entries, all of which are excluded.
        // No extension offsets or strings are dereferenced by this reader.
        Ok(Self {
            bytes,
            little_endian,
            entries_start,
            entry_size: NEW_ENTRY_SIZE,
            count,
            string_offset_base: header,
            strings: entries_end..strings_end,
        })
    }

    fn u32(&self, offset: usize) -> Result<u32> {
        read_u32(self.bytes, offset, self.little_endian)
    }

    fn u64(&self, offset: usize) -> Result<u64> {
        let bytes = read_bytes(self.bytes, offset)?;
        Ok(if self.little_endian {
            u64::from_le_bytes(bytes)
        } else {
            u64::from_be_bytes(bytes)
        })
    }

    fn string(&self, offset: u32) -> Result<&'a [u8]> {
        let offset = self
            .string_offset_base
            .checked_add(offset as usize)
            .filter(|offset| self.strings.contains(offset))
            .ok_or(ParseLdCacheError::InvalidStringOffset)?;
        CStr::from_bytes_until_nul(&self.bytes[offset..self.strings.end])
            .map(CStr::to_bytes)
            .map_err(|_| ParseLdCacheError::UnterminatedString.into())
    }
}

fn checked_table_end(start: usize, count: usize, stride: usize, available: usize) -> Result<usize> {
    count
        .checked_mul(stride)
        .and_then(|size| start.checked_add(size))
        .filter(|end| *end <= available)
        .ok_or_else(|| ParseLdCacheError::Truncated.into())
}

fn read_bytes<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    let end = offset.checked_add(N).ok_or(ParseLdCacheError::Truncated)?;
    bytes
        .get(offset..end)
        .ok_or_else(|| ParseLdCacheError::Truncated.into())
        .map(|slice| slice.try_into().expect("checked fixed-size slice"))
}

fn read_u32(bytes: &[u8], offset: usize, little_endian: bool) -> Result<u32> {
    let value = read_bytes(bytes, offset)?;
    Ok(if little_endian {
        u32::from_le_bytes(value)
    } else {
        u32::from_be_bytes(value)
    })
}
