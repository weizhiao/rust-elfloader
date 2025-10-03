//! Contains content related to the CPU instruction set
use crate::relocate_error;
use alloc::{boxed::Box, string::ToString};
use core::ops::{Add, Deref, DerefMut, Sub};
use elf::abi::{
    SHN_UNDEF, STB_GLOBAL, STB_GNU_UNIQUE, STB_LOCAL, STB_WEAK, STT_COMMON, STT_FUNC,
    STT_GNU_IFUNC, STT_NOTYPE, STT_OBJECT, STT_TLS,
};

cfg_if::cfg_if! {
    if #[cfg(target_arch = "x86_64")]{
        pub(crate) type  StaticRelocator = X86_64Relocator;
    }else {
        pub(crate) type  StaticRelocator = DummyRelocator;
        pub(crate) struct DummyRelocator;
        pub(crate) const PLT_ENTRY_SIZE: usize = 16;
        pub(crate) const LAZY_PLT_HEADER_SIZE: usize = 32;
        pub(crate) const PLT_HEADER_SIZE: usize = 0;
        pub(crate) const LAZY_PLT_ENTRY_SIZE: usize = 16;

        const LAZY_PLT_HEADER: [u8; LAZY_PLT_HEADER_SIZE] = [
            0xf3, 0x0f, 0x1e, 0xfa, // endbr64
            0x41, 0x53, // push %r11
            0xff, 0x35, 0, 0, 0, 0, // push GOTPLT+8(%rip)
            0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+16(%rip)
            0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            0xcc, 0xcc, 0xcc, 0xcc, // (padding)
            0xcc, 0xcc, // (padding)
        ];

        pub(crate) const LAZY_PLT_ENTRY: [u8; LAZY_PLT_ENTRY_SIZE] = [
            0xf3, 0x0f, 0x1e, 0xfa, // endbr64
            0x41, 0xbb, 0, 0, 0, 0, // mov $index_in_relplt, %r11d
            0xff, 0x25, 0, 0, 0, 0, // jmp *foo@GOTPLT
        ];

        pub(crate) const PLT_ENTRY: [u8; PLT_ENTRY_SIZE] = [
            0xf3, 0x0f, 0x1e, 0xfa, // endbr64
            0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+idx(%rip)
            0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, // (padding)
        ];

        impl crate::relocation::static_link::StaticReloc for DummyRelocator {
            fn relocate<F>(
                _core: &crate::CoreComponent,
                _rel_type: &ElfRelType,
                _pltgot: &mut crate::segment::shdr::PltGotSection,
                _scope: &[&crate::Relocated],
                _pre_find: &F,
            ) -> crate::Result<()>
            where
                F: Fn(&str) -> Option<*const ()>,
            {
                todo!()
            }
        }

        impl crate::segment::shdr::PltGotSection{
            pub(crate) fn init_pltgot(&mut self) {
                todo!()
            }
        }
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_arch = "x86_64")]{
        mod x86_64;
        pub use x86_64::*;
    }else if #[cfg(target_arch = "riscv64")]{
        mod riscv64;
        pub use riscv64::*;
    }else if #[cfg(target_arch = "riscv32")]{
        mod riscv32;
        pub use riscv32::*;
    }else if #[cfg(target_arch="aarch64")]{
        mod aarch64;
        pub use aarch64::*;
    }else if #[cfg(target_arch="loongarch64")]{
        mod loongarch64;
        pub use loongarch64::*;
    }else if #[cfg(target_arch = "x86")]{
        mod x86;
        pub use x86::*;
    }else if #[cfg(target_arch = "arm")]{
        mod arm;
        pub use arm::*;
    }
}

pub const REL_NONE: u32 = 0;
const OK_BINDS: usize = 1 << STB_GLOBAL | 1 << STB_WEAK | 1 << STB_GNU_UNIQUE;
const OK_TYPES: usize = 1 << STT_NOTYPE
    | 1 << STT_OBJECT
    | 1 << STT_FUNC
    | 1 << STT_COMMON
    | 1 << STT_TLS
    | 1 << STT_GNU_IFUNC;

cfg_if::cfg_if! {
    if #[cfg(target_pointer_width = "64")]{
        pub(crate) const E_CLASS: u8 = elf::abi::ELFCLASS64;
        pub(crate) type Phdr = elf::segment::Elf64_Phdr;
        pub(crate) type Shdr = elf::section::Elf64_Shdr;
        pub type Dyn = elf::dynamic::Elf64_Dyn;
        pub(crate) type Ehdr = elf::file::Elf64_Ehdr;
        pub(crate) type Rela = elf::relocation::Elf64_Rela;
        pub(crate) type Rel = elf::relocation::Elf64_Rel;
        pub(crate) type Relr = u64;
        pub(crate) type Sym = elf::symbol::Elf64_Sym;
        pub(crate) const REL_MASK: usize = 0xFFFFFFFF;
        pub(crate) const REL_BIT: usize = 32;
        pub(crate) const EHDR_SIZE: usize = core::mem::size_of::<elf::file::Elf64_Ehdr>();
    }else{
        pub(crate) const E_CLASS: u8 = elf::abi::ELFCLASS32;
        pub(crate) type Phdr = elf::segment::Elf32_Phdr;
        pub(crate) type Shdr = elf::section::Elf32_Shdr;
        pub type Dyn = elf::dynamic::Elf32_Dyn;
        pub(crate) type Ehdr = elf::file::Elf32_Ehdr;
        pub(crate) type Rela = elf::relocation::Elf32_Rela;
        pub(crate) type Rel = elf::relocation::Elf32_Rel;
        pub(crate) type Relr = u32;
        pub(crate) type Sym = Elf32Sym;
        pub(crate) const REL_MASK: usize = 0xFF;
        pub(crate) const REL_BIT: usize = 8;
        pub(crate) const EHDR_SIZE: usize = core::mem::size_of::<elf::file::Elf32_Ehdr>();
    }
}

#[repr(C)]
pub struct Elf32Sym {
    pub st_name: u32,
    pub st_value: u32,
    pub st_size: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
}

/// This element holds the total size, in bytes, of the DT_RELR relocation table.
pub const DT_RELRSZ: i64 = 35;
/// This element is similar to DT_RELA, except its table has implicit
/// addends and info, such as Elf32_Relr for the 32-bit file class or
/// Elf64_Relr for the 64-bit file class. If this element is present,
/// the dynamic structure must also have DT_RELRSZ and DT_RELRENT elements.
pub const DT_RELR: i64 = 36;
/// This element holds the size, in bytes, of the DT_RELR relocation entry.
pub const DT_RELRENT: i64 = 37;

#[repr(transparent)]
pub struct ElfRelr {
    relr: Relr,
}

impl ElfRelr {
    #[inline]
    pub fn value(&self) -> usize {
        self.relr as usize
    }
}

#[repr(transparent)]
pub struct ElfRela {
    rela: Rela,
}

impl ElfRela {
    #[inline]
    pub fn r_type(&self) -> usize {
        self.rela.r_info as usize & REL_MASK
    }

    #[inline]
    pub fn r_symbol(&self) -> usize {
        self.rela.r_info as usize >> REL_BIT
    }

    #[inline]
    pub fn r_offset(&self) -> usize {
        self.rela.r_offset as usize
    }

    /// base is not used during execution. The base parameter is added only for the sake of interface consistency
    #[inline]
    pub fn r_addend(&self, _base: usize) -> isize {
        self.rela.r_addend as isize
    }

    #[inline]
    pub(crate) fn set_offset(&mut self, offset: usize) {
        self.rela.r_offset = offset as _;
    }
}

#[repr(transparent)]
pub struct ElfRel {
    rel: Rel,
}

impl ElfRel {
    #[inline]
    pub fn r_type(&self) -> usize {
        self.rel.r_info as usize & REL_MASK
    }

    #[inline]
    pub fn r_symbol(&self) -> usize {
        self.rel.r_info as usize >> REL_BIT
    }

    #[inline]
    pub fn r_offset(&self) -> usize {
        self.rel.r_offset as usize
    }

    #[inline]
    pub fn r_addend(&self, base: usize) -> isize {
        let ptr = (self.r_offset() + base) as *mut usize;
        unsafe { ptr.read() as isize }
    }

	#[inline]
	pub(crate) fn set_offset(&mut self, offset: usize) {
        self.rel.r_offset = offset as _;
    }
}

#[repr(transparent)]
pub struct ElfSymbol {
    sym: Sym,
}

impl ElfSymbol {
    #[inline]
    pub fn st_value(&self) -> usize {
        self.sym.st_value as usize
    }

    /// STB_* define constants for the ELF Symbol's st_bind (encoded in the st_info field)
    #[inline]
    pub fn st_bind(&self) -> u8 {
        self.sym.st_info >> 4
    }

    /// STT_* define constants for the ELF Symbol's st_type (encoded in the st_info field).
    #[inline]
    pub fn st_type(&self) -> u8 {
        self.sym.st_info & 0xf
    }

    #[inline]
    pub fn st_shndx(&self) -> usize {
        self.sym.st_shndx as usize
    }

    #[inline]
    pub fn st_name(&self) -> usize {
        self.sym.st_name as usize
    }

    #[inline]
    pub fn st_size(&self) -> usize {
        self.sym.st_size as usize
    }

    #[inline]
    pub fn st_other(&self) -> u8 {
        self.sym.st_other
    }

    #[inline]
    pub fn is_undef(&self) -> bool {
        self.st_shndx() == SHN_UNDEF as usize
    }

    #[inline]
    pub fn is_ok_bind(&self) -> bool {
        (1 << self.st_bind()) & OK_BINDS != 0
    }

    #[inline]
    pub fn is_ok_type(&self) -> bool {
        (1 << self.st_type()) & OK_TYPES != 0
    }

    #[inline]
    pub fn is_local(&self) -> bool {
        self.st_bind() == STB_LOCAL
    }

    #[inline]
    pub fn is_weak(&self) -> bool {
        self.st_bind() == STB_WEAK
    }

    #[inline]
    pub(crate) fn set_value(&mut self, value: usize) {
        self.sym.st_value = value as _;
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct ElfPhdr {
    phdr: Phdr,
}

#[derive(Debug)]
#[repr(transparent)]
pub struct ElfShdr {
    shdr: Shdr,
}

impl ElfShdr {
    pub(crate) fn new(
        sh_name: u32,
        sh_type: u32,
        sh_flags: usize,
        sh_addr: usize,
        sh_offset: usize,
        sh_size: usize,
        sh_link: u32,
        sh_info: u32,
        sh_addralign: usize,
        sh_entsize: usize,
    ) -> Self {
        Self {
            shdr: Shdr {
                sh_name,
                sh_type,
                sh_flags: sh_flags as _,
                sh_addr: sh_addr as _,
                sh_offset: sh_offset as _,
                sh_size: sh_size as _,
                sh_link,
                sh_info,
                sh_addralign: sh_addralign as _,
                sh_entsize: sh_entsize as _,
            },
        }
    }
}

impl Deref for ElfShdr {
    type Target = Shdr;

    fn deref(&self) -> &Self::Target {
        &self.shdr
    }
}

impl DerefMut for ElfShdr {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.shdr
    }
}

impl ElfShdr {
    pub(crate) fn content<T>(&self) -> &'static [T] {
        self.content_mut()
    }

    pub(crate) fn content_mut<T>(&self) -> &'static mut [T] {
        let start = self.sh_addr as usize;
        let len = (self.sh_size / self.sh_entsize) as usize;
        debug_assert!(core::mem::size_of::<T>() == self.sh_entsize as usize);
        debug_assert!(self.sh_size % self.sh_entsize == 0);
        debug_assert!(self.sh_addr % self.sh_addralign == 0);
        unsafe { core::slice::from_raw_parts_mut(start as *mut T, len) }
    }
}

impl Deref for ElfPhdr {
    type Target = Phdr;

    fn deref(&self) -> &Self::Target {
        &self.phdr
    }
}

impl Clone for ElfPhdr {
    fn clone(&self) -> Self {
        Self {
            phdr: Phdr {
                p_type: self.phdr.p_type,
                p_flags: self.phdr.p_flags,
                p_align: self.phdr.p_align,
                p_offset: self.phdr.p_offset,
                p_vaddr: self.phdr.p_vaddr,
                p_paddr: self.phdr.p_paddr,
                p_filesz: self.phdr.p_filesz,
                p_memsz: self.phdr.p_memsz,
            },
        }
    }
}

#[inline]
pub(crate) fn prepare_lazy_bind(got: *mut usize, dylib: usize) {
    // 这是安全的，延迟绑定时库是存在的
    unsafe {
        got.add(DYLIB_OFFSET).write(dylib);
        got.add(RESOLVE_FUNCTION_OFFSET)
            .write(dl_runtime_resolve as usize);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RelocValue(pub usize);

impl Add<isize> for RelocValue {
    type Output = RelocValue;

    fn add(self, rhs: isize) -> Self::Output {
        RelocValue(self.0.wrapping_add_signed(rhs))
    }
}

impl Sub<usize> for RelocValue {
    type Output = RelocValue;
    fn sub(self, rhs: usize) -> Self::Output {
        RelocValue(self.0.wrapping_sub(rhs))
    }
}

impl From<RelocValue> for usize {
    fn from(value: RelocValue) -> Self {
        value.0
    }
}

impl TryFrom<RelocValue> for i32 {
    type Error = crate::Error;

    fn try_from(value: RelocValue) -> Result<Self, Self::Error> {
        i32::try_from(value.0 as isize).map_err(|err| relocate_error(err.to_string(), Box::new(())))
    }
}

#[cfg(not(feature = "rel"))]
pub type ElfRelType = ElfRela;
#[cfg(feature = "rel")]
pub type ElfRelType = ElfRel;
