use crate::{
    Result,
    arch::{
        ElfShdr, LAZY_PLT_ENTRY, LAZY_PLT_ENTRY_SIZE, LAZY_PLT_HEADER_SIZE, PLT_ENTRY,
        PLT_ENTRY_SIZE, PLT_HEADER_SIZE, RelocValue, Shdr,
    },
    mmap::{MapFlags, Mmap, ProtFlags},
    segment::{
        Address, ElfSegment, ElfSegments, FileMapInfo, PAGE_SIZE, SegmentBuilder, rounddown,
        roundup,
    },
};
use alloc::vec::Vec;
use elf::abi::{
    SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE, SHT_INIT_ARRAY, SHT_NOBITS, SHT_REL, SHT_RELA,
};
use hashbrown::{HashMap, hash_map::Entry};

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
    pub(crate) fn new(shdrs: &mut [ElfShdr], lazy_bind: bool) -> Self {
        // Create section units for different memory protection types
        let mut units: [SectionUnit; 4] = core::array::from_fn(|_| SectionUnit::new());

        // Create special sections for GOT, PLT, and PLTGOT
        let mut got_shdr = PltGotSection::create_got_shdr(shdrs);
        let (mut pltgot_shdr, mut plt_shdr) = PltGotSection::create_plt_shdr(shdrs, lazy_bind);

        // Group sections by their protection flags
        for shdr in shdrs.iter_mut() {
            units[flags_to_idx(shdr.sh_flags.into())].add_section(shdr);
        }

        // Add special sections to their respective units
        units[flags_to_idx(got_shdr.sh_flags.into())].add_section(&mut got_shdr);
        units[flags_to_idx(plt_shdr.sh_flags.into())].add_section(&mut plt_shdr);
        units[flags_to_idx(pltgot_shdr.sh_flags.into())].add_section(&mut pltgot_shdr);

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
            pltgot: Some(PltGotSection::new(
                &got_shdr,
                &plt_shdr,
                &pltgot_shdr,
                lazy_bind,
            )),
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
    pltgot_base: usize,             // Base address of PLTGOT
    cur_got_idx: usize,             // Current index in GOT
    cur_plt_idx: usize,             // Current index in PLT
    got_map: HashMap<usize, usize>, // Map from symbol index to GOT entry index
    plt_map: HashMap<usize, usize>, // Map from symbol index to PLT entry index
    got_size: usize,                // Size of GOT section
    plt_size: usize,                // Size of PLT section
    pltgot_size: usize,             // Size of PLTGOT section
    plt_header_size: usize,         // Size of PLT header
    plt_entry_size: usize,          // Size of each PLT entry
    lazy_bind: bool,                // Whether to use lazy binding
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
        plt: &'plt mut [u8],      // PLT entry data
        pltgot: UsizeEntry<'plt>, // Corresponding PLTGOT entry
    },
}

impl PltGotSection {
    /// Create a GOT section header based on relocation entries
    fn create_got_shdr(shdrs: &[ElfShdr]) -> ElfShdr {
        // TODO: optimize to reduce the size
        let mut elem_cnt = 0;
        // Count total number of relocation entries
        for shdr in shdrs.iter() {
            if shdr.sh_type == SHT_REL || shdr.sh_type == SHT_RELA {
                elem_cnt += (shdr.sh_size / shdr.sh_entsize) as usize;
            }
        }

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
    fn create_plt_shdr(shdrs: &[ElfShdr], lazy: bool) -> (ElfShdr, ElfShdr) {
        // TODO: optimize to reduce the size
        // Count total number of relocation entries
        let mut elem_cnt = 0;
        for shdr in shdrs.iter() {
            if shdr.sh_type == SHT_REL || shdr.sh_type == SHT_RELA {
                elem_cnt += (shdr.sh_size / shdr.sh_entsize) as usize;
            }
        }

        // Determine PLT header and entry sizes based on lazy binding
        let header_size = if lazy {
            LAZY_PLT_HEADER_SIZE
        } else {
            PLT_HEADER_SIZE
        };
        let entry_size = if lazy {
            LAZY_PLT_ENTRY_SIZE
        } else {
            PLT_ENTRY_SIZE
        };

        // Create GOT section (for PLT targets)
        let got_section = ElfShdr::new(
            0,
            SHT_NOBITS,
            (SHF_ALLOC | SHF_WRITE) as _,
            0,
            0,
            elem_cnt * size_of::<usize>(),
            0,
            0,
            size_of::<usize>(),
            size_of::<usize>(),
        );

        // Create PLT section (executable code stubs)
        let plt_section = ElfShdr::new(
            0,
            SHT_NOBITS,
            (SHF_ALLOC | SHF_EXECINSTR) as _,
            0,
            0,
            elem_cnt * entry_size + header_size,
            0,
            0,
            size_of::<usize>(),
            entry_size,
        );

        (got_section, plt_section)
    }

    /// Create a new PltGotSection instance
    fn new(got: &Shdr, plt: &Shdr, pltgot: &Shdr, lazy: bool) -> Self {
        Self {
            cur_got_idx: 0,
            cur_plt_idx: 0,
            got_map: HashMap::new(),
            plt_map: HashMap::new(),
            got_base: got.sh_addr as usize,
            plt_base: plt.sh_addr as usize + LAZY_PLT_HEADER_SIZE,
            pltgot_base: pltgot.sh_addr as usize,
            got_size: got.sh_size as usize,
            plt_size: plt.sh_size as usize,
            pltgot_size: pltgot.sh_size as usize,
            plt_entry_size: plt.sh_entsize as usize,
            plt_header_size: if lazy {
                LAZY_PLT_HEADER_SIZE
            } else {
                PLT_HEADER_SIZE
            },
            lazy_bind: lazy,
        }
    }

    /// Adjust base addresses by adding an offset (used during relocation)
    pub(crate) fn rebase(&mut self, base: usize) {
        self.got_base = self.got_base + base;
        self.plt_base = self.plt_base + base;
        self.pltgot_base = self.pltgot_base + base;
    }

    /// Get the base address of the PLT section
    pub(crate) fn plt_base(&self) -> usize {
        self.plt_base
    }

    /// Get the base address of the PLTGOT section
    pub(crate) fn pltgot_base(&self) -> usize {
        self.pltgot_base
    }

    /// Check if lazy binding is enabled
    pub(crate) fn is_lazy(&self) -> bool {
        self.lazy_bind
    }

    /// Get the PLT header as a mutable byte slice
    pub(crate) fn get_plt_header(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                (self.plt_base - self.plt_header_size) as *mut u8,
                self.plt_header_size,
            )
        }
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
                let idx = *entry.insert(self.cur_got_idx);
                self.cur_got_idx += 1;
                GotEntry::Vacant(unsafe {
                    UsizeEntry(&mut *((idx * ent_size + base) as *mut usize))
                })
            }
        }
    }

    /// Add or retrieve a PLT entry for a symbol
    pub(crate) fn add_plt_entry(&mut self, r_sym: usize) -> PltEntry<'_> {
        let plt_base = self.plt_base;
        let pltgot_base = self.pltgot_base;
        let plt_ent_size = self.plt_entry_size;
        let got_ent_size = size_of::<usize>();
        match self.plt_map.entry(r_sym) {
            Entry::Occupied(mut entry) => {
                // Return existing PLT entry
                PltEntry::Occupied(RelocValue(*entry.get_mut() * plt_ent_size + plt_base))
            }
            Entry::Vacant(entry) => {
                // Create new PLT entry
                let idx = *entry.insert(self.cur_plt_idx);
                let plt = unsafe {
                    core::slice::from_raw_parts_mut(
                        (idx * plt_ent_size + plt_base) as *mut u8,
                        plt_ent_size,
                    )
                };

                // Copy the appropriate PLT entry template
                if self.lazy_bind {
                    plt.copy_from_slice(&LAZY_PLT_ENTRY);
                } else {
                    plt.copy_from_slice(&PLT_ENTRY);
                }

                self.cur_plt_idx += 1;
                PltEntry::Vacant {
                    plt,
                    pltgot: unsafe {
                        UsizeEntry(&mut *((idx * got_ent_size + pltgot_base) as *mut usize))
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
