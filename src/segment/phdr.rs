use crate::{
    Result,
    arch::ElfPhdr,
    mmap::{MapFlags, Mmap, ProtFlags},
    segment::{
        Address, ElfSegment, ElfSegments, FileMapInfo, PAGE_SIZE, SegmentBuilder, rounddown,
        roundup,
    },
};
use alloc::vec::Vec;
use elf::abi::{PF_R, PF_W, PF_X, PT_LOAD};

#[inline]
fn segment_prot(p_flag: u32) -> ProtFlags {
    ProtFlags::from_bits_retain(((p_flag & PF_X) << 2 | p_flag & PF_W | (p_flag & PF_R) >> 2) as _)
}

pub(crate) struct PhdrSegments<'phdr> {
    phdrs: &'phdr [ElfPhdr],
    segments: Vec<ElfSegment>,
    is_dylib: bool,
    use_file: bool,
}

impl<'phdr> PhdrSegments<'phdr> {
    pub(crate) fn new(phdrs: &'phdr [ElfPhdr], is_dylib: bool, use_file: bool) -> Self {
        Self {
            phdrs,
            segments: Vec::new(),
            is_dylib,
            use_file,
        }
    }
}

#[inline]
fn parse_segments(phdrs: &[ElfPhdr], is_dylib: bool) -> (Option<usize>, usize, usize) {
    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0;
    //找到最小的偏移地址和最大的偏移地址
    for phdr in phdrs {
        if phdr.p_type == PT_LOAD {
            let vaddr_start = phdr.p_vaddr as usize;
            let vaddr_end = (phdr.p_vaddr + phdr.p_memsz) as usize;
            if vaddr_start < min_vaddr {
                min_vaddr = vaddr_start;
            }
            if vaddr_end > max_vaddr {
                max_vaddr = vaddr_end;
            }
        }
    }
    // 按页对齐
    max_vaddr = roundup(max_vaddr, PAGE_SIZE);
    min_vaddr = rounddown(min_vaddr, PAGE_SIZE);
    let total_size = max_vaddr - min_vaddr;
    (
        if is_dylib { None } else { Some(min_vaddr) },
        total_size,
        min_vaddr,
    )
}

impl SegmentBuilder for PhdrSegments<'_> {
    fn create_space<M: Mmap>(&mut self) -> Result<ElfSegments> {
        let (addr, len, min_vaddr) = parse_segments(self.phdrs, self.is_dylib);
        let ptr = unsafe { M::mmap_reserve(addr, len, self.use_file) }?;
        Ok(ElfSegments {
            memory: ptr,
            offset: min_vaddr,
            len,
            munmap: M::munmap,
        })
    }

    fn create_segments(&mut self) -> Result<()> {
        for phdr in self.phdrs {
            if phdr.p_type == PT_LOAD {
                self.segments.push(phdr.create_segment());
            }
        }
        Ok(())
    }

    fn segments_mut(&mut self) -> &mut [ElfSegment] {
        &mut self.segments
    }

    fn segments(&self) -> &[ElfSegment] {
        &self.segments
    }
}

impl ElfPhdr {
    #[inline]
    fn create_segment(&self) -> ElfSegment {
        // 映射的起始地址与结束地址都是页对齐的
        let min_vaddr = rounddown(self.p_vaddr as usize, PAGE_SIZE);
        let max_vaddr = roundup((self.p_vaddr + self.p_memsz) as usize, PAGE_SIZE);
        let memsz = max_vaddr - min_vaddr;
        let prot = segment_prot(self.p_flags);

        let offset = rounddown(self.p_offset as usize, PAGE_SIZE);
        // 因为读取是从offset处开始的，所以为了不少从文件中读数据，这里需要加上因为对齐产生的偏差
        let align_len = self.p_offset as usize - offset;
        let filesz = self.p_filesz as usize + align_len;

        ElfSegment {
            addr: Address::Relative(min_vaddr),
            prot,
            flags: MapFlags::MAP_PRIVATE | MapFlags::MAP_FIXED,
            align: self.p_align as usize,
            len: memsz,
            content_size: filesz,
            zero_size: (self.p_memsz - self.p_filesz) as usize,
            map_info: alloc::vec![FileMapInfo {
                start: 0,
                filesz,
                offset,
            }],
            need_copy: false,
            from_relocatable: false,
        }
    }
}
