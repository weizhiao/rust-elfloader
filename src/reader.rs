//! ELF object representations and access traits
//!
//! This module provides traits and implementations for accessing ELF objects,
//! whether they are stored in memory or in files. It abstracts the data source
//! to allow uniform handling of different ELF object types during the loading
//! and relocation process.

use crate::{Result, os::RawFile};
use alloc::ffi::CString;
use core::ffi::CStr;

/// A trait representing a source of ELF data that can be read from.
///
/// This trait provides a uniform interface for accessing ELF data regardless
/// of its storage medium (memory, file, etc.). Implementors of this trait
/// can be used as data sources for ELF loading operations.
///
/// The trait is designed to abstract away the underlying storage mechanism
/// while providing efficient access to the ELF data. This allows the ELF
/// loader to work with various data sources without needing to know the
/// specifics of how the data is stored or accessed.
pub trait ElfReader {
    /// Returns the name of the ELF object.
    ///
    /// This is typically the file path or a descriptive name for the object.
    /// The name is used for error reporting and debugging purposes.
    ///
    /// # Returns
    /// A C string reference containing the object's name.
    fn file_name(&self) -> &CStr;

    /// Reads data from the ELF object into a buffer.
    ///
    /// This method reads a specified number of bytes from the ELF object
    /// starting at a given offset and copies them into the provided buffer.
    ///
    /// # Arguments
    /// * `buf` - A mutable slice where the read data will be stored.
    ///           The length of this buffer determines how many bytes to read.
    /// * `offset` - The byte offset within the ELF object where reading begins.
    ///
    /// # Returns
    /// * `Ok(())` - If the read operation was successful.
    /// * `Err` - If the read operation failed (e.g., I/O error, invalid offset).
    ///
    /// # Examples
    /// ```rust,ignore
    /// let mut buffer = [0u8; 64];
    /// elf_object.read(&mut buffer, 0x100)?; // Read 64 bytes starting at offset 0x100
    /// ```
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()>;

    /// Extracts the raw file descriptor, if available.
    ///
    /// For file-based ELF objects, this method returns the underlying file
    /// descriptor which can be used for memory mapping operations. For
    /// memory-based objects, this method returns `None`.
    ///
    /// This is particularly useful for optimizing file I/O by enabling
    /// memory mapping when possible, rather than reading file contents
    /// into buffers.
    ///
    /// # Returns
    /// * `Some(fd)` - The raw file descriptor if the object is file-backed.
    /// * `None` - If the object is not file-backed (e.g., memory-based).
    fn as_fd(&self) -> Option<isize>;
}

/// An ELF object stored in memory.
///
/// This struct represents an ELF object that is entirely stored in memory
/// as a byte slice. It is useful for loading ELF files that have already
/// been read into memory or for loading ELF data from embedded resources.
///
/// Memory-based ELF objects are typically faster to access than file-based
/// ones since they don't require disk I/O operations. However, they consume
/// memory proportional to the size of the ELF file.
#[derive(Debug)]
pub struct ElfBinary<'bytes> {
    /// The name of the ELF object, typically the original file path.
    name: CString,
    /// The ELF data stored in memory as a byte slice.
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
            name: CString::new(name).unwrap(),
            bytes,
        }
    }
}

impl<'bytes> ElfReader for ElfBinary<'bytes> {
    /// Returns the name of the ELF binary.
    ///
    /// # Returns
    /// A C string reference containing the binary's name.
    fn file_name(&self) -> &CStr {
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

/// An ELF object backed by a file.
///
/// This struct represents an ELF object that is stored in a file and accessed
/// through file I/O operations. It wraps a [RawFile] to provide the [ElfObject]
/// interface for file-based ELF data.
///
/// File-based ELF objects are useful for loading large ELF files without
/// consuming large amounts of memory. They support memory mapping optimizations
/// when a file descriptor is available.
pub struct ElfFile {
    /// The underlying raw file abstraction.
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
    fn file_name(&self) -> &CStr {
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
