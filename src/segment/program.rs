use crate::{
    ParsePhdrError, Result,
    elf::{ElfLayout, ElfPhdr, ElfProgramFlags, ElfProgramType},
    input::{ElfReader, check_read_range},
    memory::{VmAddr, VmOffset, rounddown, roundup},
    os::{Mmap, ProtFlags},
    segment::{ElfSegment, ElfSegments, SegmentContent, populate_segments, protect_segments},
};

/// Convert ELF program header flags to memory protection flags
#[inline]
fn segment_prot(flags: ElfProgramFlags) -> ProtFlags {
    let mut prot = ProtFlags::PROT_NONE;
    if flags.contains(ElfProgramFlags::READ) {
        prot |= ProtFlags::PROT_READ;
    }
    if flags.contains(ElfProgramFlags::WRITE) {
        prot |= ProtFlags::PROT_WRITE;
    }
    if flags.contains(ElfProgramFlags::EXEC) {
        prot |= ProtFlags::PROT_EXEC;
    }
    prot
}

/// Validates program headers, creates the address space, and populates its segments.
pub(crate) fn load_segments<M>(
    phdrs: &[ElfPhdr<impl ElfLayout>],
    is_dylib: bool,
    mapper: &M,
    object: &impl ElfReader,
    page_size: usize,
) -> Result<ElfSegments<M::Region>>
where
    M: Mmap + ?Sized,
{
    for phdr in phdrs {
        if phdr.program_type() == ElfProgramType::LOAD && phdr.p_filesz() != 0 {
            check_read_range(phdr.p_offset(), phdr.p_filesz(), object.len())?;
        }
    }
    let file_backing = match object.file_backing() {
        Some(range) => mapper.file_backing(range)?,
        None => None,
    };
    let layout = parse_segments(phdrs, is_dylib, page_size)?;
    let segments = phdrs
        .iter()
        .filter(|phdr| phdr.program_type() == ElfProgramType::LOAD)
        .map(|phdr| phdr.create_segment(page_size));

    let use_file = file_backing.is_some();
    let prot = if use_file {
        ProtFlags::PROT_NONE
    } else {
        ProtFlags::PROT_WRITE
    };
    let region =
        unsafe { mapper.create_space(layout.preferred_addr, layout.mapped_len, prot, use_file) }?;
    let base = region.addr() - layout.min_vaddr;
    let loaded = ElfSegments::new(region, base, layout.min_vaddr);
    populate_segments(mapper, object, &loaded, segments.clone(), file_backing)?;
    if !use_file {
        protect_segments(&loaded, segments)?;
    }
    Ok(loaded)
}

/// Memory layout requirements derived from LOAD program headers.
pub(crate) struct ProgramSegmentLayout {
    /// Preferred address passed to `mmap`; `None` lets the OS choose.
    preferred_addr: Option<VmAddr>,
    /// Total page-aligned memory range covered by LOAD segments.
    pub(crate) mapped_len: usize,
    /// Lowest page-aligned virtual address among LOAD segments.
    pub(crate) min_vaddr: VmOffset,
}

/// Parse segments to determine memory layout requirements
#[inline]
pub(crate) fn parse_segments(
    phdrs: &[ElfPhdr<impl ElfLayout>],
    is_dylib: bool,
    page_size: usize,
) -> Result<ProgramSegmentLayout> {
    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0;
    let mut has_load_segment = false;

    // Find the minimum and maximum virtual addresses of LOAD segments
    for phdr in phdrs {
        if phdr.program_type() == ElfProgramType::LOAD {
            let vaddr_start = phdr.p_vaddr();
            if vaddr_start.get() % page_size != phdr.p_offset() % page_size {
                return Err(ParsePhdrError::PageAlignmentMismatch { page_size }.into());
            }

            has_load_segment = true;
            if phdr.p_filesz() > phdr.p_memsz() {
                return Err(ParsePhdrError::malformed("PT_LOAD p_filesz exceeds p_memsz").into());
            }

            let vaddr_end = vaddr_start
                .checked_add(phdr.p_memsz())
                .map(VmOffset::get)
                .ok_or(ParsePhdrError::malformed(
                    "PT_LOAD virtual address range overflows",
                ))?;
            let vaddr_start = vaddr_start.get();
            if vaddr_start < min_vaddr {
                min_vaddr = vaddr_start;
            }
            if vaddr_end > max_vaddr {
                max_vaddr = vaddr_end;
            }
        }
    }

    if !has_load_segment {
        return Err(ParsePhdrError::malformed("program headers do not contain PT_LOAD").into());
    }

    // Align addresses to page boundaries
    max_vaddr = roundup(max_vaddr, page_size);
    min_vaddr = rounddown(min_vaddr, page_size);
    let total_size = max_vaddr
        .checked_sub(min_vaddr)
        .ok_or(ParsePhdrError::malformed(
            "PT_LOAD virtual address range is inverted",
        ))?;

    // For shared libraries, let the OS choose the base address (None)
    // For executables, suggest the preferred base address (Some)
    Ok(ProgramSegmentLayout {
        preferred_addr: if is_dylib {
            None
        } else {
            Some(VmAddr::new(min_vaddr))
        },
        mapped_len: total_size,
        min_vaddr: VmOffset::new(min_vaddr),
    })
}

impl<L: ElfLayout> ElfPhdr<L> {
    /// Create an ElfSegment from an ELF program header
    #[inline]
    fn create_segment(&self, page_size: usize) -> ElfSegment {
        // Align segment boundaries to page size
        let p_vaddr = self.p_vaddr().get();
        let min_vaddr = rounddown(p_vaddr, page_size);
        let max_vaddr = roundup(p_vaddr + self.p_memsz(), page_size);
        let memsz = max_vaddr - min_vaddr;
        let prot = segment_prot(self.flags());

        // Align file offset to page boundary
        let offset = rounddown(self.p_offset(), page_size);
        // Account for alignment adjustment in file size
        let align_len = self.p_offset() - offset;
        let filesz = self.p_filesz() + align_len;

        ElfSegment {
            offset: VmOffset::new(min_vaddr),
            prot,
            len: memsz,
            page_size,
            content: SegmentContent::Contiguous {
                file_offset: offset,
                file_size: filesz,
                zero_size: self.p_memsz() - self.p_filesz(),
            },
        }
    }
}
