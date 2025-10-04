//! Memory mapping operations for ELF loader
//!
//! This module provides traits and implementations for memory mapping operations
//! required by the ELF loader. It abstracts platform-specific memory management
//! operations to provide a unified interface.

pub use crate::os::MmapImpl;

use crate::Result;
use bitflags::bitflags;
use core::{
    ffi::{c_int, c_void},
    ptr::NonNull,
};

bitflags! {
    #[derive(Clone, Copy, Debug, Default)]
    /// Memory protection flags for memory mapping operations.
    ///
    /// These flags control the access permissions for mapped memory regions.
    /// They can be combined using bitwise operations to specify multiple
    /// permissions simultaneously.
    pub struct ProtFlags: c_int {
        /// No access permissions. Pages cannot be accessed.
        /// Attempts to read, write, or execute will result in a protection fault.
        const PROT_NONE = 0;

        /// Read permission. Pages can be read from.
        /// This is the most basic permission typically required for executable code
        /// and read-only data.
        const PROT_READ = 1;

        /// Write permission. Pages can be written to.
        /// Required for data sections that need to be modified at runtime,
        /// such as the Global Offset Table (GOT) in position-independent code.
        const PROT_WRITE = 2;

        /// Execute permission. Pages can be executed as code.
        /// Required for code sections and the Procedure Linkage Table (PLT).
        /// Note that some systems implement W^X (write XOR execute) security policies
        /// that prevent pages from having both write and execute permissions simultaneously.
        const PROT_EXEC = 4;
    }
}

bitflags! {
    #[derive(Clone, Copy)]
    /// Mapping flags that control the behavior of memory mapping operations.
    ///
    /// These flags determine how the mapping is created and managed by the system.
    /// They control aspects such as sharing, fixed positioning, and backing storage.
    pub struct MapFlags: c_int {
        /// Create a private copy-on-write mapping.
        ///
        /// Changes to the mapping are private to the process and do not affect
        /// the underlying file or other processes. Mutually exclusive with `MAP_SHARED`.
        /// This is typically used for executable code and read-only data.
        const MAP_PRIVATE = 2;

        /// Place the mapping at exactly the specified address.
        ///
        /// The mapping will be created at the exact address provided, replacing
        /// any existing mappings. If the address is not available, the operation
        /// will fail. This is essential for ELF loading where segments need to
        /// be placed at specific virtual addresses.
        const MAP_FIXED = 16;

        /// Create an anonymous mapping not backed by any file.
        ///
        /// The mapping is initialized to zero and is not associated with any file.
        /// This is used for allocating memory that doesn't correspond to file content,
        /// such as BSS sections or heap memory.
        const MAP_ANONYMOUS = 32;
    }
}

/// A trait representing low-level memory mapping operations.
///
/// This trait encapsulates the functionality for memory-mapped file I/O and
/// anonymous memory mapping. It provides unsafe methods to map, unmap, and
/// protect memory regions, as well as to create anonymous memory mappings.
///
/// The trait is designed to abstract platform-specific memory management
/// operations, allowing the ELF loader to work across different operating
/// systems and environments.
///
/// # Safety
/// All methods in this trait are unsafe because they directly manipulate
/// the virtual address space of the process. Incorrect usage can lead to
/// memory corruption, segmentation faults, or security vulnerabilities.
///
/// # Examples
/// To use this trait, implement it for a specific type that represents a
/// memory mapping facility. The implementations would handle the
/// platform-specific details of memory management.
///
/// ```rust,ignore
/// struct MyMmap;
///
/// unsafe impl Mmap for MyMmap {
///     unsafe fn mmap(
///         addr: Option<usize>,
///         len: usize,
///         prot: ProtFlags,
///         flags: MapFlags,
///         offset: usize,
///         fd: Option<isize>,
///         need_copy: &mut bool,
///     ) -> Result<NonNull<c_void>> {
///         // Platform-specific implementation
///     }
///     
///     // ... other methods
/// }
/// ```
pub trait Mmap {
    /// Maps a file or bytes into memory at the specified address.
    ///
    /// This function creates a mapping between a file (or anonymous memory)
    /// and the process's virtual address space. The mapping can be configured
    /// with various protection and mapping flags.
    ///
    /// # Arguments
    /// * `addr` - An optional starting address for the mapping.
    ///   - If `Some(address)`, the system attempts to create the mapping at that address
    ///   - If `None`, the system chooses a suitable address
    ///   Note: The address is always aligned to page size (typically 4096 bytes).
    /// * `len` - The length of the memory region to map in bytes.
    ///   Will be rounded up to the nearest page size boundary.
    /// * `prot` - The protection options for the mapping (e.g., readable, writable, executable).
    ///   See [ProtFlags] for available options.
    /// * `flags` - The flags controlling the details of the mapping (e.g., shared, private).
    ///   See [MapFlags] for available options.
    /// * `offset` - The byte offset within the file where the mapping begins.
    ///   Must be a multiple of the page size.
    /// * `fd` - An optional file descriptor for file-backed mappings.
    ///   - If `Some(fd)`, the mapping is backed by the file associated with the descriptor
    ///   - If `None`, the mapping is anonymous (not backed by any file)
    /// * `need_copy` - A mutable boolean that indicates whether the caller needs to
    ///   manually copy data to the mapped region.
    ///   - Set to `false` if the mmap function can handle segment copying internally
    ///   - Set to `true` if the caller must manually copy data to the mapped region
    ///
    /// # Returns
    /// * `Ok(NonNull<c_void>)` - A non-null pointer to the mapped memory region
    /// * `Err` - If the mapping operation fails
    ///
    /// # Safety
    /// This function is unsafe because it directly manipulates the process's
    /// virtual address space. The caller must ensure:
    /// - The provided file descriptor is valid (if provided)
    /// - The offset is properly aligned
    /// - The memory region does not conflict with existing mappings (unless using MAP_FIXED)
    /// - Proper synchronization if the mapping is shared between threads or processes
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
        offset: usize,
        fd: Option<isize>,
        need_copy: &mut bool,
    ) -> Result<NonNull<c_void>>;

    /// Creates a new anonymous mapping with the specified protection and flags.
    ///
    /// This function creates a mapping that is not backed by any file. The
    /// mapping is initialized to zero and can be used for dynamic memory allocation.
    ///
    /// # Arguments
    /// * `addr` - The starting address for the mapping.
    ///   - If non-zero, the system attempts to create the mapping at that address
    ///   - If zero, the system chooses a suitable address
    /// * `len` - The length of the memory region to map in bytes.
    ///   Will be rounded up to the nearest page size boundary.
    /// * `prot` - The protection options for the mapping.
    ///   See [ProtFlags] for available options.
    /// * `flags` - The flags controlling the details of the mapping.
    ///   See [MapFlags] for available options.
    ///   Note: MAP_ANONYMOUS is implicitly added to these flags.
    ///
    /// # Returns
    /// * `Ok(NonNull<c_void>)` - A non-null pointer to the mapped memory region
    /// * `Err` - If the mapping operation fails
    ///
    /// # Safety
    /// This function is unsafe because it directly manipulates the process's
    /// virtual address space. The caller must ensure proper use of the
    /// returned memory region.
    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
    ) -> Result<NonNull<c_void>>;

    /// Releases a previously mapped memory region.
    ///
    /// This function unmaps a previously created memory mapping, making the
    /// memory region available for future allocations. After this call,
    /// accessing the memory region results in undefined behavior.
    ///
    /// # Arguments
    /// * `addr` - A non-null pointer to the start of the memory region to unmap.
    ///   Must be the exact address returned by a previous mapping operation.
    /// * `len` - The length of the memory region to unmap in bytes.
    ///   Should match the length specified in the original mapping operation.
    ///
    /// # Returns
    /// * `Ok(())` - If the unmapping operation succeeds
    /// * `Err` - If the unmapping operation fails
    ///
    /// # Safety
    /// This function is unsafe because it invalidates the memory region.
    /// The caller must ensure:
    /// - No pointers into the unmapped region are used after this call
    /// - The address and length match a previous mapping operation
    /// - No other threads are accessing the region during the operation
    unsafe fn munmap(addr: NonNull<c_void>, len: usize) -> Result<()>;

    /// Changes the protection of a memory region.
    ///
    /// This function alters the protection options for a mapped memory region.
    /// It can be used to make a region readable, writable, executable, or any
    /// combination thereof.
    ///
    /// This is commonly used in ELF loading for RELRO (RELocation Read-Only)
    /// protection, where certain sections are made read-only after relocations
    /// have been applied.
    ///
    /// # Arguments
    /// * `addr` - A non-null pointer to the start of the memory region to protect.
    ///   Must be page-aligned.
    /// * `len` - The length of the memory region to protect in bytes.
    ///   Will be rounded up to the nearest page size boundary.
    /// * `prot` - The new protection options for the mapping.
    ///   See [ProtFlags] for available options.
    ///
    /// # Returns
    /// * `Ok(())` - If the protection change succeeds
    /// * `Err` - If the protection change fails
    ///
    /// # Safety
    /// This function is unsafe because it changes memory access permissions.
    /// The caller must ensure:
    /// - The address range is currently mapped
    /// - The new permissions are compatible with the underlying memory
    /// - No threads are accessing the region in a way that conflicts with
    ///   the new permissions
    unsafe fn mprotect(addr: NonNull<c_void>, len: usize, prot: ProtFlags) -> Result<()>;

    /// Reserves a region of memory for future use without committing physical storage.
    ///
    /// This function reserves a memory region in the process's virtual address space
    /// but does not allocate physical memory nor create any actual mapping. The reserved
    /// region can later be committed with additional mapping operations.
    ///
    /// This is particularly useful for ELF loading where the total memory footprint
    /// is known in advance, allowing the loader to reserve the entire address space
    /// before creating individual segment mappings.
    ///
    /// The default implementation creates a mapping with PROT_NONE protection,
    /// which reserves the address space without committing physical memory.
    ///
    /// # Arguments
    /// * `addr` - An optional starting address for the reservation.
    ///   - If `Some(address)`, the system attempts to reserve memory at the specified address
    ///   - If `None`, the system chooses an appropriate address
    /// * `len` - The length of the memory region to reserve in bytes.
    ///   Will be rounded up to the nearest page size boundary.
    /// * `_use_file` - A flag indicating whether the reserved region will eventually
    ///   be backed by a file. This parameter may be used by platform-specific implementations.
    ///
    /// # Returns
    /// * `Ok(NonNull<c_void>)` - A non-null pointer to the reserved memory region
    /// * `Err` - If the reservation fails
    ///
    /// # Safety
    /// This function is unsafe because it manipulates the process's virtual address space.
    /// Accessing the reserved memory region before it is properly mapped results in
    /// a segmentation fault.
    unsafe fn mmap_reserve(
        addr: Option<usize>,
        len: usize,
        _use_file: bool,
    ) -> Result<NonNull<c_void>> {
        let mut need_copy = false;
        // Create a reservation by mapping with PROT_NONE
        // This reserves the address space without committing physical memory
        unsafe {
            Self::mmap(
                addr,
                len,
                ProtFlags::PROT_NONE,
                MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS,
                0,
                None,
                &mut need_copy,
            )
        }
    }
}
