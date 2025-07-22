// https://loongson.github.io/LoongArch-Documentation/LoongArch-ELF-ABI-CN.html

const EM_LARCH: u16 = 258;
const R_LARCH_64: u32 = 2;
const R_LARCH_RELATIVE: u32 = 3;
const R_LARCH_COPY: u32 = 4;
const R_LARCH_JUMP_SLOT: u32 = 5;
const R_LARCH_TLS_DTPMOD64: u32 = 7;
const R_LARCH_TLS_DTPREL64: u32 = 9;
const R_LARCH_TLS_TPREL64: u32 = 11;
const R_LARCH_IRELATIVE: u32 = 12;

pub const EM_ARCH: u16 = EM_LARCH;
pub const TLS_DTV_OFFSET: usize = 0;

pub const REL_SYMBOLIC: u32 = R_LARCH_64;
pub const REL_RELATIVE: u32 = R_LARCH_RELATIVE;
pub const REL_COPY: u32 = R_LARCH_COPY;
pub const REL_JUMP_SLOT: u32 = R_LARCH_JUMP_SLOT;
pub const REL_DTPMOD: u32 = R_LARCH_TLS_DTPMOD64;
pub const REL_DTPOFF: u32 = R_LARCH_TLS_DTPREL64;
pub const REL_IRELATIVE: u32 = R_LARCH_IRELATIVE;
pub const REL_TPOFF: u32 = R_LARCH_TLS_TPREL64;

pub const REL_GOT: u32 = u32::MAX;

pub(crate) const DYLIB_OFFSET: usize = 1;
pub(crate) const RESOLVE_FUNCTION_OFFSET: usize = 0;

#[unsafe(naked)]
pub extern "C" fn dl_runtime_resolve() {
    core::arch::naked_asm!(
        "
	"
    )
}
