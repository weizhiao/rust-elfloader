use super::{MadviseAdvice, MapFlags, PageSize, ProtFlags};
use crate::memory::{MappedRegion, RegionAccess, VmAddr};
use crate::{Result, input::FileBacking};
use alloc::sync::Arc;

/// Creates and manages memory mappings for loaded images.
///
/// [`Self::create_space`] returns the owner of a mapping. File and zero-page
/// operations fill subranges without taking ownership. Implementations must
/// support concurrent calls and report mapping failures.
pub trait Mmap: Send + Sync + 'static {
    /// Region type created by this mapping backend.
    type Region: RegionAccess;

    /// Returns the base page size required by this mapping environment.
    ///
    /// Implementations that can query the host should return the active system
    /// page size. Bare-metal or syscall-only implementations can keep the
    /// default 4 KiB base page.
    #[inline]
    fn page_size(&self) -> PageSize {
        PageSize::Base
    }

    /// Selects direct mapping (`Some`) or reading and copying (`None`, the default).
    ///
    /// An accepted range must contain the same bytes as the input. This query does
    /// not map memory or take ownership of the file handle. Invalid descriptions
    /// return errors; later mapping failures do not fall back to copying.
    #[inline]
    fn file_backing(&self, _backing: FileBacking) -> Result<Option<FileBacking>> {
        Ok(None)
    }

    /// Creates an owned address range of `len` bytes, preferring `addr` if supplied.
    ///
    /// If `populate_later` is false, zero the entire range and apply `prot`, including
    /// for allocator-backed or caller-owned storage. Callers writing into it must
    /// request writable protection. Otherwise, reservation alone is sufficient;
    /// populate file or zero pages before accessing them.
    ///
    /// # Safety
    /// `addr` must be page-aligned. Caller-owned storage must remain valid and be
    /// exclusively available for initialization and loading.
    unsafe fn create_space(
        &self,
        addr: Option<VmAddr>,
        len: usize,
        prot: ProtFlags,
        populate_later: bool,
    ) -> Result<MappedRegion<Self::Region>>;

    /// Borrows an existing mapping without changing its contents or unmapping it on drop.
    /// The view should support protection changes when the backend allows them.
    ///
    /// # Safety
    /// `addr..addr + len` must remain mapped for the returned region's lifetime.
    unsafe fn alias_space(&self, addr: VmAddr, len: usize) -> Result<MappedRegion<Self::Region>>;

    /// Maps file contents into a subrange owned by [`Self::create_space`].
    /// The backend must prepare the subrange, splitting reservations if needed.
    /// Unsupported operations must return an error.
    ///
    /// # Safety
    /// `addr..addr + len` must be within a region created by this mapper.
    /// `fd` and `offset` must identify a valid readable file range of `len` bytes.
    unsafe fn map_file_at(
        &self,
        addr: VmAddr,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
        offset: usize,
        fd: isize,
    ) -> Result<()>;

    /// Maps or commits anonymous zero-filled pages into an already-created space.
    ///
    /// The mapped range is owned by the surrounding space created with
    /// [`Mmap::create_space`]. The backend must prepare the target subrange,
    /// including splitting address reservations when the platform requires it.
    ///
    /// # Safety
    /// `addr..addr + len` must be inside a region previously created by this
    /// mapper and must not overlap incompatible existing mappings.
    unsafe fn map_zero_at(
        &self,
        addr: VmAddr,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
    ) -> Result<()>;

    /// Releases a mapping created by [`Self::create_space`].
    ///
    /// # Safety
    /// `addr` must be page-aligned, and `addr` and `len` must match the original
    /// mapping. No code may access the region after it is unmapped.
    unsafe fn munmap(&self, addr: VmAddr, len: usize) -> Result<()>;

    /// Applies memory-usage advice to a mapped range.
    /// Unsupported advice may return an error.
    ///
    /// # Safety
    /// `addr` must be page-aligned. The caller must account for the effects of
    /// `behavior`, including advice that discards or frees memory contents.
    unsafe fn madvise(&self, addr: VmAddr, len: usize, behavior: MadviseAdvice) -> Result<()>;

    /// Changes access permissions for `len` bytes, rounded up to a page boundary.
    /// Used, for example, to make RELRO memory read-only after relocation.
    ///
    /// # Safety
    /// `addr` must be page-aligned and identify a valid mapping. Ensure all users
    /// of the range permit the change; no code may execute there when execute
    /// permission is removed.
    unsafe fn mprotect(&self, addr: VmAddr, len: usize, prot: ProtFlags) -> Result<()>;
}

impl<M> Mmap for Arc<M>
where
    M: Mmap + ?Sized,
{
    type Region = M::Region;

    #[inline]
    fn page_size(&self) -> PageSize {
        (**self).page_size()
    }

    #[inline]
    fn file_backing(&self, backing: FileBacking) -> Result<Option<FileBacking>> {
        (**self).file_backing(backing)
    }

    #[inline]
    unsafe fn create_space(
        &self,
        addr: Option<VmAddr>,
        len: usize,
        prot: ProtFlags,
        populate_later: bool,
    ) -> Result<MappedRegion<Self::Region>> {
        unsafe { (**self).create_space(addr, len, prot, populate_later) }
    }

    #[inline]
    unsafe fn alias_space(&self, addr: VmAddr, len: usize) -> Result<MappedRegion<Self::Region>> {
        unsafe { (**self).alias_space(addr, len) }
    }

    #[inline]
    unsafe fn map_file_at(
        &self,
        addr: VmAddr,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
        offset: usize,
        fd: isize,
    ) -> Result<()> {
        unsafe { (**self).map_file_at(addr, len, prot, flags, offset, fd) }
    }

    #[inline]
    unsafe fn map_zero_at(
        &self,
        addr: VmAddr,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
    ) -> Result<()> {
        unsafe { (**self).map_zero_at(addr, len, prot, flags) }
    }

    #[inline]
    unsafe fn munmap(&self, addr: VmAddr, len: usize) -> Result<()> {
        unsafe { (**self).munmap(addr, len) }
    }

    #[inline]
    unsafe fn madvise(&self, addr: VmAddr, len: usize, behavior: MadviseAdvice) -> Result<()> {
        unsafe { (**self).madvise(addr, len, behavior) }
    }

    #[inline]
    unsafe fn mprotect(&self, addr: VmAddr, len: usize, prot: ProtFlags) -> Result<()> {
        unsafe { (**self).mprotect(addr, len, prot) }
    }
}
