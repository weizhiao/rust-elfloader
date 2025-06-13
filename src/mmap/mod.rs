//! Map memory to address space

#[cfg(all(feature = "use-libc", feature = "use-syscall"))]
compile_error!("only one of use-libc and use-syscall can be used");

#[cfg(all(
    any(feature = "fs", feature = "mmap"),
    not(any(feature = "use-libc", feature = "use-syscall")),
    not(target_os = "windows")
))]
compile_error!("use at least one of libc and syscall");

cfg_if::cfg_if! {
    if #[cfg(target_os = "windows")]{
        pub(crate) mod windows;
        pub use windows::MmapImpl;
    }else if #[cfg(all(feature = "mmap", feature = "use-libc"))]{
        pub(crate) mod libc;
        pub use libc::MmapImpl;
    }else if #[cfg(all(feature = "mmap", feature = "use-syscall"))]{
        pub(crate) mod linux_syscall;
        pub use linux_syscall::MmapImpl;
    }else {
        #[path = "no_mmap.rs"]
        pub(crate) mod no_mmap_impl;
        pub use no_mmap_impl::MmapImpl;
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
    #[derive(Clone, Copy)]
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

/// A trait representing low-level memory mapping operations.
///
/// This trait encapsulates the functionality for memory-mapped file I/O and anonymous memory mapping.
/// It provides unsafe methods to map, unmap, and protect memory regions, as well as to create anonymous memory mappings.
///
/// # Examples
/// To use this trait, one would typically implement it for a specific type that represents a memory mapping facility.
/// The implementations would handle the platform-specific details of memory management.
pub trait Mmap {
    /// This function maps a file or bytes into memory at the specified address with the given protection and flags.
    ///
    /// # Arguments
    /// * `addr` - An optional starting address for the mapping. The address is always aligned by page size(4096).
    /// * `len` - The length of the memory region to map. The length is always aligned by page size(4096).
    /// * `prot` - The protection options for the mapping (e.g., readable, writable, executable).
    /// * `flags` - The flags controlling the details of the mapping (e.g., shared, private).
    /// * `offset` - The file offset.
    /// * `fd` - The file descriptor.
    /// * `need_copy` - It is set to false if the mmap function can do the job of segment copying on its own, and to true otherwise.
    /// # Safety
    /// This depends on the correctness of the trait implementation.
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
        offset: usize,
        fd: Option<i32>,
        need_copy: &mut bool,
    ) -> Result<NonNull<c_void>>;

    /// This function creates a new anonymous mapping with the specified protection and flags.
    ///
    /// # Arguments
    /// * `addr` - The starting address for the mapping.
    /// * `len` - The length of the memory region to map.
    /// * `prot` - The protection options for the mapping.
    /// * `flags` - The flags controlling the details of the mapping.
    ///
    /// # Safety
    /// This depends on the correctness of the trait implementation.
    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
    ) -> Result<NonNull<c_void>>;

    /// This function releases a previously mapped memory region.
    ///
    /// # Arguments
    /// * `addr` - A `NonNull` pointer to the start of the memory region to unmap.
    /// * `len` - The length of the memory region to unmap.
    /// # Safety
    /// This depends on the correctness of the trait implementation.
    unsafe fn munmap(addr: NonNull<c_void>, len: usize) -> Result<()>;

    /// Changes the protection of a memory region.
    ///
    /// This function alters the protection options for a mapped memory region.
    ///
    /// # Arguments
    /// * `addr` - A `NonNull` pointer to the start of the memory region to protect.
    /// * `len` - The length of the memory region to protect.
    /// * `prot` - The new protection options for the mapping.
    /// # Safety
    /// This depends on the correctness of the trait implementation.
    unsafe fn mprotect(addr: NonNull<c_void>, len: usize, prot: ProtFlags) -> Result<()>;
}
