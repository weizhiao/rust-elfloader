use super::{ElfReader, IntoElfReader, ModuleSourceId, Path, PathBuf};
use crate::{IoError, ReadBoundsError, Result, logging, os::RawFile};
use alloc::{borrow::Cow, boxed::Box, string::String, vec::Vec};
use core::ops::Range;

pub(crate) fn checked_read_range(
    offset: usize,
    len: usize,
    available: usize,
) -> Result<Range<usize>> {
    let Some(end) = offset.checked_add(len).filter(|&end| end <= available) else {
        return Err(IoError::ReadOutOfBounds(Box::new(ReadBoundsError::new(
            offset, len, available,
        )))
        .into());
    };
    Ok(offset..end)
}

/// An ELF object source backed by an in-memory byte slice.
///
/// This is useful for loading ELF files that are already in memory, such as
/// those embedded in the binary or received over a network.
#[derive(Debug)]
pub struct ElfBinary<'bytes> {
    /// Loader source path or caller-provided source identifier.
    path: PathBuf,
    /// Stable identity allocated for this memory source.
    source_id: ModuleSourceId,
    /// The raw ELF data.
    bytes: Cow<'bytes, [u8]>,
}

impl<'bytes> ElfBinary<'bytes> {
    /// Creates a new memory-based ELF object from a byte slice.
    ///
    /// # Examples
    /// ```rust
    /// use elf_loader::input::ElfBinary;
    ///
    /// let data = &[]; // In practice, this would be the bytes of an ELF file
    /// let binary = ElfBinary::new("liba.so", data);
    /// ```
    pub fn new(path: impl Into<PathBuf>, bytes: &'bytes [u8]) -> Self {
        Self {
            path: path.into(),
            source_id: ModuleSourceId::fresh(),
            bytes: Cow::Borrowed(bytes),
        }
    }

    /// Creates a new memory-based ELF object that owns its bytes.
    ///
    /// This is useful for scanned images, which retain their reader until the
    /// later mapping phase.
    pub fn owned(path: impl Into<PathBuf>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            source_id: ModuleSourceId::fresh(),
            bytes: Cow::Owned(bytes.into()),
        }
    }

    /// Replaces the identity assigned to this memory source.
    #[inline]
    pub fn with_source_id(mut self, source_id: ModuleSourceId) -> Self {
        self.source_id = source_id;
        self
    }
}

impl<'bytes> ElfReader for ElfBinary<'bytes> {
    /// Returns the source path or caller-provided identifier.
    fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the byte length of the in-memory ELF object.
    fn len(&self) -> usize {
        self.bytes.as_ref().len()
    }

    /// Reads data from the memory-based ELF object.
    fn read(&self, buf: &mut [u8], offset: usize) -> Result<()> {
        let bytes = self.bytes.as_ref();
        let range = checked_read_range(offset, buf.len(), bytes.len())?;
        buf.copy_from_slice(&bytes[range]);
        Ok(())
    }

    #[inline]
    fn source_id(&self) -> ModuleSourceId {
        self.source_id
    }

    /// Borrows data directly from the memory-based ELF object.
    fn borrow_bytes(&self, offset: usize, len: usize) -> Result<Option<&[u8]>> {
        let bytes = self.bytes.as_ref();
        let range = checked_read_range(offset, len, bytes.len())?;
        Ok(Some(&bytes[range]))
    }
}

/// An owned file or file region used as an ELF source.
///
/// Reads are bounded by the view and use offsets relative to its start.
/// Direct mappings use page granularity; bytes outside the view in a boundary
/// page are not isolated from the rest of the backing file.
pub struct ElfFile {
    inner: RawFile,
    path: PathBuf,
    offset: usize,
    len: usize,
    source_id: ModuleSourceId,
}

impl ElfFile {
    fn from_raw(path: &Path, inner: RawFile) -> Result<Self> {
        let (len, source_id) = inner.metadata()?;
        Ok(Self {
            len,
            source_id,
            inner,
            path: PathBuf::from(path),
            offset: 0,
        })
    }

    /// Creates a new file-based ELF object from an owned file descriptor.
    ///
    /// # Safety
    /// The caller must ensure that `raw_fd` is valid and owned by this object.
    pub unsafe fn from_owned_fd(path: impl AsRef<Path>, raw_fd: i32) -> Result<Self> {
        let path = path.as_ref();
        Self::from_raw(path, RawFile::from_owned_fd(raw_fd)?)
    }

    /// Creates a new file-based ELF object by opening a file at the given path.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        logging::debug!("Opening ELF file: {}", path);

        let inner = RawFile::from_path(path)?;
        Self::from_raw(path, inner)
    }

    /// Restricts the current view to `offset..offset + len` and assigns its path and identity.
    ///
    /// Reuse `source_id` for the same source region; use distinct IDs for different
    /// regions or [`ModuleSourceId::fresh`] to prevent reuse. The path is descriptive.
    /// Unaligned views use copying instead of direct mapping.
    pub fn into_range(
        mut self,
        path: impl Into<PathBuf>,
        offset: usize,
        len: usize,
        source_id: ModuleSourceId,
    ) -> Result<Self> {
        checked_read_range(offset, len, self.len)?;
        self.offset += offset;
        self.len = len;
        self.path = path.into();
        self.source_id = source_id;
        Ok(self)
    }
}

impl ElfReader for ElfFile {
    /// Returns the path of the ELF file.
    fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the byte length of the file-backed ELF object.
    fn len(&self) -> usize {
        self.len
    }

    /// Reads data from the file-based ELF object.
    fn read(&self, buf: &mut [u8], offset: usize) -> Result<()> {
        checked_read_range(offset, buf.len(), self.len)?;
        self.inner.read(buf, self.offset + offset)
    }

    #[inline]
    fn source_id(&self) -> ModuleSourceId {
        self.source_id
    }

    fn file_backing(&self) -> Option<super::FileBacking> {
        Some(super::FileBacking {
            fd: self.inner.mapping_handle(),
            offset: self.offset,
            len: self.len,
        })
    }
}

#[repr(align(8))]
struct FilePrefix([u8; 1024]);

/// Reads the usual ELF and program headers together for filesystem resolution.
/// The prefix stays inline in the resolver's existing boxed reader.
pub(crate) struct BufferedElfFile {
    file: ElfFile,
    prefix: FilePrefix,
    prefix_len: usize,
}

impl BufferedElfFile {
    pub(crate) fn from_path(path: &Path) -> Result<Self> {
        let file = ElfFile::from_path(path)?;
        let mut prefix = FilePrefix([0; 1024]);
        let prefix_len = file.len().min(prefix.0.len());
        file.read(&mut prefix.0[..prefix_len], 0)?;
        Ok(Self {
            file,
            prefix,
            prefix_len,
        })
    }
}

impl ElfReader for BufferedElfFile {
    fn path(&self) -> &Path {
        self.file.path()
    }

    fn len(&self) -> usize {
        self.file.len()
    }

    fn read(&self, buf: &mut [u8], offset: usize) -> Result<()> {
        if let Some(bytes) = self.borrow_bytes(offset, buf.len())? {
            buf.copy_from_slice(bytes);
            Ok(())
        } else {
            self.file.read(buf, offset)
        }
    }

    fn source_id(&self) -> ModuleSourceId {
        self.file.source_id()
    }

    fn borrow_bytes(&self, offset: usize, len: usize) -> Result<Option<&[u8]>> {
        let range = checked_read_range(offset, len, self.file.len())?;
        Ok(self.prefix.0[..self.prefix_len].get(range))
    }

    fn file_backing(&self) -> Option<super::FileBacking> {
        self.file.file_backing()
    }
}

// Implementation for string slices (file paths)
impl IntoElfReader for &str {
    type Reader = ElfFile;

    fn into_reader(self) -> Result<Self::Reader> {
        ElfFile::from_path(self)
    }
}

// Implementation for owned strings (file paths)
impl IntoElfReader for String {
    type Reader = ElfFile;

    fn into_reader(self) -> Result<Self::Reader> {
        ElfFile::from_path(self)
    }
}

impl IntoElfReader for &Path {
    type Reader = ElfFile;

    fn into_reader(self) -> Result<Self::Reader> {
        ElfFile::from_path(self)
    }
}

#[cfg(feature = "std")]
impl IntoElfReader for &std::path::Path {
    type Reader = ElfFile;

    fn into_reader(self) -> Result<Self::Reader> {
        ElfFile::from_path(self)
    }
}

impl IntoElfReader for PathBuf {
    type Reader = ElfFile;

    fn into_reader(self) -> Result<Self::Reader> {
        ElfFile::from_path(self)
    }
}

#[cfg(feature = "std")]
impl IntoElfReader for std::path::PathBuf {
    type Reader = ElfFile;

    fn into_reader(self) -> Result<Self::Reader> {
        ElfFile::from_path(self)
    }
}

impl IntoElfReader for &PathBuf {
    type Reader = ElfFile;

    fn into_reader(self) -> Result<Self::Reader> {
        ElfFile::from_path(self)
    }
}

// Implementation for byte slices (in-memory ELF data)
impl<'a> IntoElfReader for &'a [u8] {
    type Reader = ElfBinary<'a>;

    fn into_reader(self) -> Result<Self::Reader> {
        Ok(ElfBinary::new("<memory>", self))
    }
}

impl<'a> IntoElfReader for &'a Vec<u8> {
    type Reader = ElfBinary<'a>;

    fn into_reader(self) -> Result<Self::Reader> {
        Ok(ElfBinary::new("<memory>", self.as_slice()))
    }
}

impl IntoElfReader for Vec<u8> {
    type Reader = ElfBinary<'static>;

    fn into_reader(self) -> Result<Self::Reader> {
        Ok(ElfBinary::owned("<memory>", self))
    }
}

impl IntoElfReader for Box<[u8]> {
    type Reader = ElfBinary<'static>;

    fn into_reader(self) -> Result<Self::Reader> {
        Ok(ElfBinary::owned("<memory>", self.into_vec()))
    }
}
