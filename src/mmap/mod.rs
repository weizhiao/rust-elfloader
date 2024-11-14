cfg_if::cfg_if! {
    if #[cfg(feature = "mmap")]{
        pub(crate) mod mmap;
        pub use mmap::MmapImpl;
    }else {
        pub(crate) mod no_mmap;
        pub use no_mmap::MmapImpl;
    }
}

use crate::Result;
use bitflags::bitflags;
use core::{
    ffi::{c_int, c_void},
    ptr::NonNull,
};

bitflags! {
    #[derive(Clone, Copy)]
    /// Desired memory protection of a memory mapping.
    pub struct ProtFlags: c_int {
        /// Pages cannot be accessed.
        const PROT_NONE = 0;
        /// Pages can be read.
        const PROT_READ = 1;
        /// Pages can be written.
        const PROT_WRITE = 2;
        /// Pages can be executed
        const PROT_EXEC = 4;
    }
}

bitflags! {
     /// Additional parameters for [`mmap`].
     pub struct MapFlags: c_int {
        /// Create a private copy-on-write mapping. Mutually exclusive with `MAP_SHARED`.
        const MAP_PRIVATE = 2;
        /// Place the mapping at exactly the address specified in `addr`.
        const MAP_FIXED = 16;
        /// The mapping is not backed by any file.
        const MAP_ANONYMOUS = 32;
    }
}

/// Represents the type of offset used for memory mapping operations.
///
/// This enum distinguishes between offsets based on a file descriptor and offsets based on a raw address.
pub enum OffsetType {
    /// An offset type that is based on a file descriptor and a file offset.
    ///
    /// This variant is used when the memory mapping operation is related to a file.
    #[cfg(feature = "std")]
    File {
        /// The file descriptor associated with the file.
        fd: c_int,
        /// The offset within the file from which to start the mapping.
        file_offset: usize,
    },
    /// An offset type that is based on a raw address.
    ///
    /// This variant is used when the memory mapping operation is related to an anonymous memory region or an address.
    Addr(*const u8),
}

/// Represents an offset along with its length for memory mapping operations.
///
/// This struct is used to specify the offset and length for memory-mapped regions.
pub struct Offset {
    /// The offset of the mapping content relative to the addr
    pub align_offset: usize,
    /// The length of the memory region to be mapped.
    pub len: usize,
    /// The type of offset, which can be either a file descriptor-based offset or an address-based offset.
    pub kind: OffsetType,
}

/// A trait representing low-level memory mapping operations.
///
/// This trait encapsulates the functionality for memory-mapped file I/O and anonymous memory mapping.
/// It provides unsafe methods to map, unmap, and protect memory regions, as well as to create anonymous memory mappings.
///
/// # Examples
/// To use this trait, one would typically implement it for a specific type that represents a memory mapping facility.
/// The implementations would handle the platform-specific details of memory management.
///
/// # Note
/// The `ProtFlags`, `MapFlags`, and `Offset` types are expected to be defined elsewhere in the module,
/// and are used here to specify the protection options, mapping flags, and offset for the memory mapping operations.
pub trait Mmap {
    /// Maps a memory region into the process's address space.
    ///
    /// This function maps a file or device into memory at the specified address with the given protection and flags.
    /// If the `addr` is `None`, the kernel will choose an address at which to map the memory.
    ///
    /// # Arguments
    /// * `addr` - An optional starting address for the mapping.
    /// * `len` - The length of the memory region to map.
    /// * `prot` - The protection options for the mapping (e.g., readable, writable, executable).
    /// * `flags` - The flags controlling the details of the mapping (e.g., shared, private).
    /// * `offset` - The offset into the file or device from which to start the mapping.
    ///
    /// # Returns
    /// A `Result` containing a `NonNull` pointer to the mapped memory if successful, or an error if the operation fails.
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
        offset: Offset,
    ) -> Result<NonNull<c_void>>;

    /// Maps an anonymous memory region into the process's address space.
    ///
    /// This function creates a new anonymous mapping with the specified protection and flags.
    ///
    /// # Arguments
    /// * `addr` - The starting address for the mapping.
    /// * `len` - The length of the memory region to map.
    /// * `prot` - The protection options for the mapping.
    /// * `flags` - The flags controlling the details of the mapping.
    ///
    /// # Returns
    /// A `Result` containing a `NonNull` pointer to the mapped memory if successful, or an error if the operation fails.
    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
    ) -> Result<NonNull<c_void>>;

    /// Unmaps a memory region from the process's address space.
    ///
    /// This function releases a previously mapped memory region.
    ///
    /// # Arguments
    /// * `addr` - A `NonNull` pointer to the start of the memory region to unmap.
    /// * `len` - The length of the memory region to unmap.
    ///
    /// # Returns
    /// A `Result` that is `Ok` if the operation succeeds, or an error if it fails.
    unsafe fn munmap(addr: NonNull<c_void>, len: usize) -> Result<()>;

    /// Changes the protection of a memory region.
    ///
    /// This function alters the protection options for a mapped memory region.
    ///
    /// # Arguments
    /// * `addr` - A `NonNull` pointer to the start of the memory region to protect.
    /// * `len` - The length of the memory region to protect.
    /// * `prot` - The new protection options for the mapping.
    ///
    /// # Returns
    /// A `Result` that is `Ok` if the operation succeeds, or an error if it fails.
    unsafe fn mprotect(addr: NonNull<c_void>, len: usize, prot: ProtFlags) -> Result<()>;
}
