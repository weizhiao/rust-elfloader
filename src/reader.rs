//! ELF object representations and access traits
//!
//! This module provides traits and implementations for accessing ELF objects,
//! whether they are stored in memory or in files. It abstracts the data source
//! to allow uniform handling of different ELF object types during the loading
//! and relocation process.

use crate::{Result, os::RawFile};
use alloc::string::{String, ToString};

/// A trait for reading ELF data from various sources.
///
/// `ElfReader` abstracts the underlying storage (memory, file system, etc.)
/// providing a unified interface for the loader to access ELF headers and segments.
pub trait ElfReader {
    /// Returns the full name or path of the ELF object.
    fn file_name(&self) -> &str;

    /// Reads a chunk of data from the ELF object into the provided buffer.
    ///
    /// # Arguments
    /// * `buf` - The destination buffer. Its length determines the number of bytes read.
    /// * `offset` - The starting byte offset within the ELF source.
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()>;

    /// Returns the underlying file descriptor if the source is a file.
    ///
    /// This is used by the loader to perform efficient memory mapping (`mmap`).
    /// Returns `None` for memory-based sources.
    fn as_fd(&self) -> Option<isize>;

    /// Returns the short name of the ELF object (the filename without the path).
    fn shortname(&self) -> &str {
        let name = self.file_name();
        name.rsplit('/').next().unwrap_or(name)
    }
}

/// An ELF object source backed by an in-memory byte slice.
///
/// This is useful for loading ELF files that are already in memory, such as
/// those embedded in the binary or received over a network.
#[derive(Debug)]
pub struct ElfBinary<'bytes> {
    /// The name assigned to this ELF object.
    name: String,
    /// The raw ELF data.
    bytes: &'bytes [u8],
}

impl<'bytes> ElfBinary<'bytes> {
    /// Creates a new memory-based ELF object.
    ///
    /// This constructor creates an [`ElfBinary`] instance from a byte slice
    /// containing the ELF data and a name for the object.
    ///
    /// # Arguments
    /// * `name` - A string identifier for the ELF object, typically the
    ///            original file path. Used for error reporting and debugging.
    /// * `bytes` - A byte slice containing the complete ELF data.
    ///
    /// # Returns
    /// A new [`ElfBinary`] instance.
    ///
    /// # Examples
    /// ```rust
    /// use elf_loader::ElfBinary;
    ///
    /// let data = &[]; // In practice, this would be the bytes of an ELF file
    /// let binary = ElfBinary::new("liba.so", data);
    /// ```
    pub fn new(name: &str, bytes: &'bytes [u8]) -> Self {
        Self {
            name: name.to_string(),
            bytes,
        }
    }
}

impl<'bytes> ElfReader for ElfBinary<'bytes> {
    /// Returns the name of the ELF binary.
    ///
    /// # Returns
    /// A string slice containing the binary's name.
    fn file_name(&self) -> &str {
        &self.name
    }

    /// Reads data from the memory-based ELF object.
    ///
    /// This implementation directly copies data from the in-memory byte slice
    /// to the provided buffer, making it very efficient.
    ///
    /// # Arguments
    /// * `buf` - A mutable slice where the read data will be stored.
    /// * `offset` - The byte offset within the ELF data where reading begins.
    ///
    /// # Returns
    /// * `Ok(())` - If the read operation was successful.
    /// * `Err` - If the read operation would go beyond the available data.
    fn read(&mut self, buf: &mut [u8], offset: usize) -> crate::Result<()> {
        buf.copy_from_slice(&self.bytes[offset..offset + buf.len()]);
        Ok(())
    }

    /// Returns None since memory-based objects don't have file descriptors.
    ///
    /// # Returns
    /// Always returns `None` for memory-based ELF objects.
    fn as_fd(&self) -> Option<isize> {
        None
    }
}


/// An ELF object source backed by a file on the filesystem.
///
/// This implementation uses standard file I/O to read ELF data. It also
/// provides access to the underlying file descriptor for memory mapping.
pub struct ElfFile {
    /// The underlying OS-specific file handle.
    inner: RawFile,
}

impl ElfFile {
    /// Creates a new file-based ELF object from an owned file descriptor.
    ///
    /// # Safety
    /// The caller must ensure that:
    /// * The `raw_fd` parameter is a valid, open file descriptor.
    /// * The file descriptor is owned by this object and will not be closed
    ///   by any other code while this object exists.
    /// * The file contains valid ELF data.
    ///
    /// # Arguments
    /// * `path` - The file path, used for identification and error reporting.
    /// * `raw_fd` - The raw file descriptor for the open ELF file.
    ///
    /// # Returns
    /// A new [ElfFile] instance.
    pub unsafe fn from_owned_fd(path: &str, raw_fd: i32) -> Self {
        ElfFile {
            inner: RawFile::from_owned_fd(path, raw_fd),
        }
    }

    /// Creates a new file-based ELF object by opening a file at the given path.
    ///
    /// This constructor opens the file at the specified path and prepares it
    /// for use as an ELF object. The file is automatically closed when the
    /// [ElfFile] instance is dropped.
    ///
    /// # Arguments
    /// * `path` - The path to the ELF file to open.
    ///
    /// # Returns
    /// * `Ok(ElfFile)` - If the file was successfully opened and is accessible.
    /// * `Err` - If the file could not be opened or accessed.
    pub fn from_path(path: impl AsRef<str>) -> Result<Self> {
        Ok(ElfFile {
            inner: RawFile::from_path(path.as_ref())?,
        })
    }
}

impl ElfReader for ElfFile {
    /// Returns the name of the ELF file.
    ///
    /// # Returns
    /// A C string reference containing the file's path.
    fn file_name(&self) -> &str {
        self.inner.file_name()
    }

    /// Reads data from the file-based ELF object.
    ///
    /// This implementation reads data from the underlying file using standard
    /// file I/O operations. For better performance with large files, consider
    /// using memory mapping when possible.
    ///
    /// # Arguments
    /// * `buf` - A mutable slice where the read data will be stored.
    /// * `offset` - The byte offset within the file where reading begins.
    ///
    /// # Returns
    /// * `Ok(())` - If the read operation was successful.
    /// * `Err` - If the read operation failed (e.g., I/O error, invalid offset).
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()> {
        self.inner.read(buf, offset)
    }

    /// Returns the raw file descriptor for the underlying file.
    ///
    /// This enables memory mapping optimizations when available, as the
    /// file descriptor can be used directly with mmap-like operations.
    ///
    /// # Returns
    /// * `Some(fd)` - The raw file descriptor for the underlying file.
    fn as_fd(&self) -> Option<isize> {
        self.inner.as_fd()
    }
}
