use crate::{
    ElfObject, Result,
    arch::{ElfShdr, PLT_ENTRY, PLT_ENTRY_SIZE, RelocValue, Shdr, StaticRelocator},
    mmap::{MapFlags, Mmap, ProtFlags},
    relocation::static_link::StaticReloc,
    segment::{
        Address, ElfSegment, ElfSegments, FileMapInfo, PAGE_SIZE, SegmentBuilder, rounddown,
        roundup,
    },
};
use alloc::vec::Vec;
use elf::abi::{
    SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE, SHT_INIT_ARRAY, SHT_NOBITS, SHT_REL, SHT_RELA,
};
use hashbrown::{HashMap, HashSet, hash_map::Entry};

/// Convert section flags to memory protection flags
pub(crate) fn section_prot(sh_flags: u64) -> ProtFlags {
    let mut prot = ProtFlags::PROT_READ;
    if sh_flags & SHF_WRITE as u64 != 0 {
        prot |= ProtFlags::PROT_WRITE;
    }
    if sh_flags & SHF_EXECINSTR as u64 != 0 {
        prot |= ProtFlags::PROT_EXEC;
    }
    prot
}

/// Manages segments created from ELF section headers
pub(crate) struct ShdrSegments {
    segments: Vec<ElfSegment>,
    total_size: usize,
    pltgot: Option<PltGotSection>,
}

/// Convert protection flags to an index for section unit management
fn prot_to_idx(prot: ProtFlags) -> usize {
    let mut idx = 0;
    if prot.contains(ProtFlags::PROT_WRITE) {
        idx |= 0b1;
    }
    if prot.contains(ProtFlags::PROT_EXEC) {
        idx |= 0b10;
    }
    idx
}

/// Convert section flags to an index for section unit management
fn flags_to_idx(flags: u64) -> usize {
    prot_to_idx(section_prot(flags))
}

impl SegmentBuilder for ShdrSegments {
    /// Reserve memory space for all segments
    fn create_space<M: Mmap>(&mut self) -> Result<ElfSegments> {
        let len = self.total_size;
        let memory = unsafe { M::mmap_reserve(None, len, false) }?;
        Ok(ElfSegments {
            memory,
            offset: 0,
            len,
            munmap: M::munmap,
        })
    }

    /// Create individual segments from section headers
    /// In this implementation, segments are pre-created in `new`, so this is a no-op
    fn create_segments(&mut self) -> Result<()> {
        Ok(())
    }

    /// Get mutable reference to segments
    fn segments_mut(&mut self) -> &mut [ElfSegment] {
        &mut self.segments
    }

    /// Get reference to segments
    fn segments(&self) -> &[ElfSegment] {
        &self.segments
    }
}

impl ShdrSegments {
    /// Create a new ShdrSegments instance from section headers
    pub(crate) fn new(shdrs: &mut [ElfShdr], object: &mut impl ElfObject) -> Self {
        // Create section units for different memory protection types
        let mut units: [SectionUnit; 4] = core::array::from_fn(|_| SectionUnit::new());

        let (got_cnt, plt_cnt) = PltGotSection::count_needed_entries(shdrs, object);

        // Create special sections for GOT and PLT
        let mut got_shdr = PltGotSection::create_got_shdr(got_cnt);
        let mut plt_shdr = PltGotSection::create_plt_shdr(plt_cnt);

        // Group sections by their protection flags
        for shdr in shdrs.iter_mut() {
            units[flags_to_idx(shdr.sh_flags.into())].add_section(shdr);
        }

        // Add special sections to their respective units
        units[flags_to_idx(got_shdr.sh_flags.into())].add_section(&mut got_shdr);
        units[flags_to_idx(plt_shdr.sh_flags.into())].add_section(&mut plt_shdr);

        // Create segments from section units
        let mut segments = Vec::new();
        let mut offset = 0;
        for unit in units.iter_mut() {
            if let Some(segment) = unit.create_segment(&mut offset) {
                offset = roundup(offset, PAGE_SIZE);
                segments.push(segment);
            }
        }

        Self {
            segments,
            total_size: offset,
            pltgot: Some(PltGotSection::new(&got_shdr, &plt_shdr)),
        }
    }

    /// Take ownership of the PLTGOT section
    pub(crate) fn take_pltgot(&mut self) -> PltGotSection {
        self.pltgot.take().unwrap()
    }
}

/// Manages PLT (Procedure Linkage Table) and GOT (Global Offset Table) sections
pub(crate) struct PltGotSection {
    got_base: usize,                // Base address of GOT
    plt_base: usize,                // Base address of PLT
    cur_idx: usize,                 // Current index in GOT
    got_map: HashMap<usize, usize>, // Map from symbol index to GOT entry index
    plt_map: HashMap<usize, usize>, // Map from symbol index to PLT entry index
}

/// Wrapper for a mutable usize value
pub(crate) struct UsizeEntry<'entry>(&'entry mut usize);

impl UsizeEntry<'_> {
    /// Update the value pointed to by this entry
    pub(crate) fn update(&mut self, value: RelocValue) {
        *self.0 = value.into();
    }

    /// Get the address of the value pointed to by this entry
    pub(crate) fn get_addr(&self) -> RelocValue {
        RelocValue(self.0 as *const _ as usize)
    }
}

/// Represents a GOT entry that may or may not be occupied
pub(crate) enum GotEntry<'got> {
    /// Entry is already occupied with a value
    Occupied(RelocValue),
    /// Entry is vacant and can be filled
    Vacant(UsizeEntry<'got>),
}

/// Represents a PLT entry that may or may not be occupied
pub(crate) enum PltEntry<'plt> {
    /// Entry is already occupied with a value
    Occupied(RelocValue),
    /// Entry is vacant and can be filled
    Vacant {
        plt: &'plt mut [u8],   // PLT entry data
        got: UsizeEntry<'plt>, // Corresponding PLTGOT entry
    },
}

impl PltGotSection {
    fn count_needed_entries(shdrs: &[ElfShdr], object: &mut impl ElfObject) -> (usize, usize) {
        let mut got_set = HashSet::new();
        let mut plt_set = HashSet::new();

        for shdr in shdrs {
            if !matches!(shdr.sh_type, SHT_REL | SHT_RELA) {
                continue;
            }

            let size = shdr.sh_size as usize;
            let entsize = shdr.sh_entsize as usize;
            if size == 0 || entsize == 0 {
                continue;
            }

            let mut buf = alloc::vec![0u8; size];
            if object.read(&mut buf, shdr.sh_offset as usize).is_err() {
                continue;
            }

            for chunk in buf.chunks(entsize) {
                if chunk.len() < entsize {
                    break;
                }
                // Safety: we read bytes from file, and we are casting to a POD type (ElfRelType)
                // We use read_unaligned to handle potential misalignment.
                let rel_entry = unsafe {
                    core::ptr::read_unaligned(chunk.as_ptr() as *const crate::arch::ElfRelType)
                };
                let r_type = rel_entry.r_type() as u32;
                let r_sym = rel_entry.r_symbol();

                if StaticRelocator::needs_got(r_type) {
                    got_set.insert(r_sym);
                }
                if StaticRelocator::needs_plt(r_type) {
                    plt_set.insert(r_sym);
                }
            }
        }
        (got_set.len(), plt_set.len())
    }

    /// Create a GOT section header based on relocation entries
    fn create_got_shdr(elem_cnt: usize) -> ElfShdr {
        // Create a NOBITS section for the GOT
        ElfShdr::new(
            0,
            SHT_NOBITS,
            (SHF_ALLOC | SHF_WRITE) as _,
            0,
            0,
            elem_cnt * size_of::<usize>(),
            0,
            0,
            16,
            size_of::<usize>(),
        )
    }

    /// Create PLT and PLTGOT section headers based on relocation entries
    fn create_plt_shdr(elem_cnt: usize) -> ElfShdr {
        // Create PLT section (executable code stubs)
        ElfShdr::new(
            0,
            SHT_NOBITS,
            (SHF_ALLOC | SHF_EXECINSTR) as _,
            0,
            0,
            elem_cnt * PLT_ENTRY_SIZE,
            0,
            0,
            size_of::<usize>(),
            PLT_ENTRY_SIZE,
        )
    }

    /// Create a new PltGotSection instance
    fn new(got: &Shdr, plt: &Shdr) -> Self {
        Self {
            cur_idx: 0,
            got_map: HashMap::new(),
            plt_map: HashMap::new(),
            got_base: got.sh_addr as usize,
            plt_base: plt.sh_addr as usize,
        }
    }

    /// Adjust base addresses by adding an offset (used during relocation)
    pub(crate) fn rebase(&mut self, base: usize) {
        self.got_base = self.got_base + base;
        self.plt_base = self.plt_base + base;
    }

    /// Add or retrieve a GOT entry for a symbol
    pub(crate) fn add_got_entry(&mut self, r_sym: usize) -> GotEntry<'_> {
        let base = self.got_base;
        let ent_size = size_of::<usize>();
        match self.got_map.entry(r_sym) {
            Entry::Occupied(mut entry) => {
                // Return existing GOT entry
                GotEntry::Occupied(RelocValue(*entry.get_mut() * ent_size + base))
            }
            Entry::Vacant(entry) => {
                // Create new GOT entry
                let idx = *entry.insert(self.cur_idx);
                self.cur_idx += 1;
                GotEntry::Vacant(unsafe {
                    UsizeEntry(&mut *((idx * ent_size + base) as *mut usize))
                })
            }
        }
    }

    /// Add or retrieve a PLT entry for a symbol
    pub(crate) fn add_plt_entry(&mut self, r_sym: usize) -> PltEntry<'_> {
        let plt_base = self.plt_base;
        let got_base = self.got_base;
        let plt_ent_size = PLT_ENTRY_SIZE;
        let got_ent_size = size_of::<usize>();
        match self.plt_map.entry(r_sym) {
            Entry::Occupied(mut entry) => {
                // Return existing PLT entry
                PltEntry::Occupied(RelocValue(*entry.get_mut() * plt_ent_size + plt_base))
            }
            Entry::Vacant(entry) => {
                // Create new PLT entry
                let idx = *entry.insert(self.cur_idx);
                let plt = unsafe {
                    core::slice::from_raw_parts_mut(
                        (idx * plt_ent_size + plt_base) as *mut u8,
                        plt_ent_size,
                    )
                };

                // Copy the appropriate PLT entry template
                plt.copy_from_slice(&PLT_ENTRY);

                self.cur_idx += 1;
                PltEntry::Vacant {
                    plt,
                    got: unsafe {
                        UsizeEntry(&mut *((idx * got_ent_size + got_base) as *mut usize))
                    },
                }
            }
        }
    }
}

/// Groups sections with the same memory protection requirements
struct SectionUnit<'shdr> {
    content_sections: Vec<&'shdr mut ElfShdr>, // Sections with file content
    zero_sectons: Vec<&'shdr mut ElfShdr>,     // Sections initialized to zero (NOBITS)
}

impl<'shdr> SectionUnit<'shdr> {
    /// Create a new SectionUnit
    fn new() -> Self {
        Self {
            content_sections: Vec::new(),
            zero_sectons: Vec::new(), // Note: This appears to be a typo in the original code ("zero_sectons" vs "zero_sections")
        }
    }

    /// Add a section to this unit based on its type
    fn add_section(&mut self, shdr: &'shdr mut ElfShdr) {
        // NOBITS sections and INIT_ARRAY sections are zero-initialized
        if shdr.sh_type == SHT_NOBITS || shdr.sh_type == SHT_INIT_ARRAY {
            self.zero_sectons.push(shdr);
        } else {
            self.content_sections.push(shdr);
        }
    }

    /// Calculate the maximum alignment requirement for sections in this unit
    fn align(&self) -> usize {
        let mut res = 0;
        // Find the maximum alignment requirement among all sections
        for shdr in self.content_sections.iter().chain(self.zero_sectons.iter()) {
            res = res.max(shdr.sh_addralign);
        }
        res as usize
    }

    /// Create a segment from the sections in this unit
    fn create_segment(&mut self, offset: &mut usize) -> Option<ElfSegment> {
        // Get section flags from the first section (all sections in a unit have the same flags)
        let sh_flags = if let Some(shdr) = self.content_sections.get(0).or(self.zero_sectons.get(0))
        {
            shdr.sh_flags
        } else {
            // No sections in this unit
            return None;
        };

        let align = self.align();
        debug_assert!(align <= PAGE_SIZE);
        let prot = section_prot(sh_flags.into());
        let addr = Address::Relative(*offset);

        /// Helper struct to manage memory layout calculations
        struct Cursor {
            start: usize, // Starting offset
            cur: usize,   // Current offset
        }

        impl Cursor {
            /// Create a new cursor at the given starting position
            fn new(start: usize) -> Self {
                Self { start, cur: start }
            }

            /// Align the current position to the specified boundary
            fn roundup(&mut self, align: usize) {
                self.cur = roundup(self.cur, align);
            }

            /// Advance the current position by the specified amount
            fn add(&mut self, size: usize) {
                self.cur += size;
            }

            /// Get the current position
            fn cur(&self) -> usize {
                self.cur
            }

            /// Get the offset from the starting position
            fn cur_offset(&self) -> usize {
                self.cur - self.start
            }
        }

        // Process content sections (those with file data)
        let mut cursor = Cursor::new(*offset);
        let mut map_info = Vec::new();
        for shdr in &mut self.content_sections {
            if shdr.sh_size == 0 {
                continue;
            }
            cursor.roundup(shdr.sh_addralign as usize);
            shdr.sh_addr = cursor.cur() as _;
            map_info.push(FileMapInfo {
                filesz: shdr.sh_size as usize,
                offset: shdr.sh_offset as usize,
                start: cursor.cur_offset(),
            });
            cursor.add(shdr.sh_size as usize);
        }

        // Special handling for a single content section to ensure page alignment
        if map_info.len() == 1 {
            let shdr = self
                .content_sections
                .iter_mut()
                .find(|shdr| shdr.sh_offset as usize == map_info[0].offset)
                .unwrap();
            let file_offset = rounddown(map_info[0].offset, PAGE_SIZE);
            let align_len = map_info[0].offset - file_offset;
            shdr.sh_addr = shdr.sh_addr.wrapping_add(align_len as _);
            map_info[0].filesz += align_len;
            map_info[0].offset = file_offset;
            cursor.add(align_len);
        }

        let content_size = cursor.cur_offset();

        // Process zero-initialized sections
        for shdr in &mut self.zero_sectons {
            cursor.roundup(shdr.sh_addralign as usize);
            shdr.sh_addr = cursor.cur() as _;
            cursor.add(shdr.sh_size as usize);
        }

        let zero_size = cursor.cur_offset() - content_size;
        let len = roundup(content_size + zero_size, PAGE_SIZE);

        if len == 0 {
            return None;
        }

        *offset += len;
        let segment = ElfSegment {
            addr,
            prot,
            len,
            content_size,
            zero_size,
            need_copy: false,
            flags: MapFlags::MAP_PRIVATE | MapFlags::MAP_FIXED,
            map_info,
            from_relocatable: true,
        };
        Some(segment)
    }
}
