use crate::{
    Result,
    memory::{RegionAccess, VmAddr, VmOffset},
    os::ProtFlags,
    segment::ElfSegments,
};
use alloc::vec::Vec;

/// A file range copied into a segment.
#[derive(Debug)]
pub(crate) struct CopyRegion {
    /// Offset within the input file.
    pub(crate) file_offset: usize,
    /// Destination offset within the segment.
    pub(crate) segment_offset: usize,
    /// Number of bytes to copy.
    pub(crate) len: usize,
}

/// File content used to populate a segment.
#[derive(Debug)]
pub(crate) enum SegmentContent {
    /// One contiguous file range starting at the segment's beginning.
    /// The range includes any prefix introduced by page alignment.
    Contiguous {
        file_offset: usize,
        file_size: usize,
        /// Number of zero-filled bytes following the file content.
        zero_size: usize,
    },
    /// Independently placed sections, copied into zero-initialized memory.
    #[cfg_attr(not(feature = "object"), allow(dead_code))]
    Scattered(Vec<CopyRegion>),
}

/// A page-aligned memory protection change applied after mapping.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MemoryProtection {
    /// Runtime address of the protected range.
    addr: VmAddr,
    /// Unrounded protected range length.
    len: usize,
    /// Page size used to round the protected range.
    page_size: usize,
    /// Protection flags to apply.
    prot: ProtFlags,
}

impl MemoryProtection {
    #[inline]
    pub(crate) const fn new(addr: VmAddr, len: usize, page_size: usize, prot: ProtFlags) -> Self {
        Self {
            addr,
            len,
            page_size,
            prot,
        }
    }

    #[inline]
    pub(crate) fn apply<R: RegionAccess>(&self, segments: &ElfSegments<R>) -> Result<()> {
        let start = self.addr.rounddown(self.page_size);
        let end = (self.addr + VmOffset::new(self.len)).roundup(self.page_size);
        let len = end
            .checked_offset_from(start)
            .expect("protection range end precedes its start")
            .get();
        segments.mprotect(start, len, self.prot)
    }
}

/// An ELF segment in memory
///
/// This structure represents a loaded ELF segment with all the
/// information needed to manage its memory mapping, protection,
/// and data content.
pub(crate) struct ElfSegment {
    /// Module-relative offset of the segment.
    pub(crate) offset: VmOffset,
    /// Memory protection flags for the segment
    pub(crate) prot: ProtFlags,
    /// Total length of the segment in bytes
    pub(crate) len: usize,
    /// Page size used to align this segment.
    pub(crate) page_size: usize,
    /// File content layout used for mapping or copying.
    pub(crate) content: SegmentContent,
}
