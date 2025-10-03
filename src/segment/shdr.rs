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

pub(crate) struct ShdrSegments {
    segments: Vec<ElfSegment>,
    total_size: usize,
    pltgot: Option<PltGotSection>,
}

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

fn flags_to_idx(flags: u64) -> usize {
    prot_to_idx(section_prot(flags))
}

impl SegmentBuilder for ShdrSegments {
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

    fn create_segments(&mut self) -> Result<()> {
        Ok(())
    }

    fn segments_mut(&mut self) -> &mut [ElfSegment] {
        &mut self.segments
    }

    fn segments(&self) -> &[ElfSegment] {
        &self.segments
    }
}

impl ShdrSegments {
    pub(crate) fn new(shdrs: &mut [ElfShdr], lazy_bind: bool) -> Self {
        let mut units: [SectionUnit; 4] = core::array::from_fn(|_| SectionUnit::new());
        let mut got_shdr = PltGotSection::create_got_shdr(shdrs);
        let (mut pltgot_shdr, mut plt_shdr) = PltGotSection::create_plt_shdr(shdrs, lazy_bind);
        for shdr in shdrs.iter_mut() {
            units[flags_to_idx(shdr.sh_flags.into())].add_section(shdr);
        }
        units[flags_to_idx(got_shdr.sh_flags.into())].add_section(&mut got_shdr);
        units[flags_to_idx(plt_shdr.sh_flags.into())].add_section(&mut plt_shdr);
        units[flags_to_idx(pltgot_shdr.sh_flags.into())].add_section(&mut pltgot_shdr);
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

    pub(crate) fn take_pltgot(&mut self) -> PltGotSection {
        self.pltgot.take().unwrap()
    }
}

pub(crate) struct PltGotSection {
    got_base: usize,
    plt_base: usize,
    pltgot_base: usize,
    cur_got_idx: usize,
    cur_plt_idx: usize,
    got_map: HashMap<usize, usize>,
    plt_map: HashMap<usize, usize>,
    got_size: usize,
    plt_size: usize,
    pltgot_size: usize,
    plt_header_size: usize,
    plt_entry_size: usize,
    lazy_bind: bool,
}

pub(crate) struct UsizeEntry<'entry>(&'entry mut usize);

impl UsizeEntry<'_> {
    pub(crate) fn update(&mut self, value: RelocValue) {
        *self.0 = value.into();
    }

    pub(crate) fn get_addr(&self) -> RelocValue {
        RelocValue(self.0 as *const _ as usize)
    }
}

pub(crate) enum GotEntry<'got> {
    Occupied(RelocValue),
    Vacant(UsizeEntry<'got>),
}

pub(crate) enum PltEntry<'plt> {
    Occupied(RelocValue),
    Vacant {
        plt: &'plt mut [u8],
        pltgot: UsizeEntry<'plt>,
    },
}

impl PltGotSection {
    fn create_got_shdr(shdrs: &[ElfShdr]) -> ElfShdr {
        // TODO: optmize to reduce the size
        let mut elem_cnt = 0;
        for shdr in shdrs.iter() {
            if shdr.sh_type == SHT_REL || shdr.sh_type == SHT_RELA {
                elem_cnt += (shdr.sh_size / shdr.sh_entsize) as usize;
            }
        }
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

    fn create_plt_shdr(shdrs: &[ElfShdr], lazy: bool) -> (ElfShdr, ElfShdr) {
        // TODO: optmize to reduce the size
        // is there need pltgot section?
        let mut elem_cnt = 0;
        for shdr in shdrs.iter() {
            if shdr.sh_type == SHT_REL || shdr.sh_type == SHT_RELA {
                elem_cnt += (shdr.sh_size / shdr.sh_entsize) as usize;
            }
        }
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
        (
            ElfShdr::new(
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
            ),
            ElfShdr::new(
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
            ),
        )
    }

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

    pub(crate) fn rebase(&mut self, base: usize) {
        self.got_base = self.got_base + base;
        self.plt_base = self.plt_base + base;
        self.pltgot_base = self.pltgot_base + base;
    }

    pub(crate) fn plt_base(&self) -> usize {
        self.plt_base
    }

    pub(crate) fn pltgot_base(&self) -> usize {
        self.pltgot_base
    }

    pub(crate) fn is_lazy(&self) -> bool {
        self.lazy_bind
    }

    pub(crate) fn get_plt_header(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                (self.plt_base - self.plt_header_size) as *mut u8,
                self.plt_header_size,
            )
        }
    }

    pub(crate) fn add_got_entry(&mut self, r_sym: usize) -> GotEntry<'_> {
        let base = self.got_base;
        let ent_size = size_of::<usize>();
        match self.got_map.entry(r_sym) {
            Entry::Occupied(mut entry) => {
                GotEntry::Occupied(RelocValue(*entry.get_mut() * ent_size + base))
            }
            Entry::Vacant(entry) => {
                let idx = *entry.insert(self.cur_got_idx);
                self.cur_got_idx += 1;
                GotEntry::Vacant(unsafe {
                    UsizeEntry(&mut *((idx * ent_size + base) as *mut usize))
                })
            }
        }
    }

    pub(crate) fn add_plt_entry(&mut self, r_sym: usize) -> PltEntry<'_> {
        let plt_base = self.plt_base;
        let pltgot_base = self.pltgot_base;
        let plt_ent_size = self.plt_entry_size;
        let got_ent_size = size_of::<usize>();
        match self.plt_map.entry(r_sym) {
            Entry::Occupied(mut entry) => {
                PltEntry::Occupied(RelocValue(*entry.get_mut() * plt_ent_size + plt_base))
            }
            Entry::Vacant(entry) => {
                let idx = *entry.insert(self.cur_plt_idx);
                let plt = unsafe {
                    core::slice::from_raw_parts_mut(
                        (idx * plt_ent_size + plt_base) as *mut u8,
                        plt_ent_size,
                    )
                };
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

struct SectionUnit<'shdr> {
    content_sections: Vec<&'shdr mut ElfShdr>,
    zero_sectons: Vec<&'shdr mut ElfShdr>,
}

impl<'shdr> SectionUnit<'shdr> {
    fn new() -> Self {
        Self {
            content_sections: Vec::new(),
            zero_sectons: Vec::new(),
        }
    }

    fn add_section(&mut self, shdr: &'shdr mut ElfShdr) {
        if shdr.sh_type == SHT_NOBITS || shdr.sh_type == SHT_INIT_ARRAY {
            self.zero_sectons.push(shdr);
        } else {
            self.content_sections.push(shdr);
        }
    }

    fn align(&self) -> usize {
        let mut res = 0;
        for shdr in self.content_sections.iter().chain(self.zero_sectons.iter()) {
            res = res.max(shdr.sh_addralign);
        }
        return res as usize;
    }

    fn create_segment(&mut self, offset: &mut usize) -> Option<ElfSegment> {
        let sh_flags = if let Some(shdr) = self.content_sections.get(0).or(self.zero_sectons.get(0))
        {
            shdr.sh_flags
        } else {
            return None;
        };
        let align = self.align();
        debug_assert!(align <= PAGE_SIZE);
        let prot = section_prot(sh_flags.into());
        let addr = Address::Relative(*offset);

        struct Cursor {
            start: usize,
            cur: usize,
        }

        impl Cursor {
            fn new(start: usize) -> Self {
                Self { start, cur: start }
            }

            fn roundup(&mut self, align: usize) {
                self.cur = roundup(self.cur, align);
            }

            fn add(&mut self, size: usize) {
                self.cur += size;
            }

            fn cur(&self) -> usize {
                self.cur
            }

            fn cur_offset(&self) -> usize {
                self.cur - self.start
            }
        }

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
        // If there is only one content section, we need to align it to page size.
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
