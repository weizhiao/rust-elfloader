use crate::{
    Result,
    arch::ElfShdr,
    mmap::{MapFlags, Mmap, ProtFlags},
    segment::{
        Address, ElfSegment, ElfSegments, FileMapInfo, PAGE_SIZE, SegmentBuilder, rounddown,
        roundup,
    },
};
use alloc::vec::Vec;
use elf::abi::{SHF_EXECINSTR, SHF_WRITE, SHT_NOBITS};

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

pub(crate) struct ShdrSegments<'shdr> {
    segments: Vec<ElfSegment>,
    shdrs: &'shdr [ElfShdr],
    total_size: usize,
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

impl SegmentBuilder for ShdrSegments<'_> {
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

impl<'shdr> ShdrSegments<'shdr> {
    pub(crate) fn new(shdrs: &'shdr mut [ElfShdr]) -> Self {
        let mut units: [SectionUnit; 4] = core::array::from_fn(|_| SectionUnit::new());
        for shdr in shdrs.iter_mut() {
            let prot = section_prot(shdr.sh_flags as u64);
            units[prot_to_idx(prot)].add_section(shdr);
        }
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
            shdrs,
            total_size: offset,
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
        if shdr.sh_type == SHT_NOBITS {
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
        let prot = section_prot(sh_flags);
        let addr = Address::Relative(*offset);
        let mut content_size = 0;
        let mut map_info = Vec::new();
        for shdr in &mut self.content_sections {
            *offset = roundup(*offset, shdr.sh_addralign as usize);
            shdr.sh_addr = *offset as _;
            map_info.push(FileMapInfo {
                filesz: shdr.sh_size as usize,
                offset: shdr.sh_offset as usize,
                start: content_size,
            });
            let size = shdr.sh_size as usize;
            content_size += size;
            *offset += size;
        }
        let mut zero_size = 0;
        for shdr in &mut self.zero_sectons {
            *offset = roundup(*offset, shdr.sh_addralign as usize);
            shdr.sh_addr = *offset as _;
            let size = shdr.sh_size as usize;
            zero_size += size;
            *offset += size;
        }
        let len = roundup(zero_size + content_size, PAGE_SIZE);
        if len == 0 {
            return None;
        }
        if map_info.len() == 1 {
            let file_offset = rounddown(map_info[0].offset, PAGE_SIZE);
            let align_len = map_info[0].offset - file_offset;
            map_info[0].filesz += align_len;
            map_info[0].offset = file_offset;
        }
        let segment = ElfSegment {
            addr,
            align,
            prot,
            len,
            content_size,
            zero_size,
            need_copy: false,
            flags: MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS,
            map_info,
        };
        Some(segment)
    }
}
