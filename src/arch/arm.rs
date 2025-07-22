use elf::abi::*;

pub const EM_ARCH: u16 = EM_ARM;
pub const TLS_DTV_OFFSET: usize = 0;

pub const REL_RELATIVE: u32 = R_ARM_RELATIVE;
pub const REL_GOT: u32 = R_ARM_GLOB_DAT;
pub const REL_DTPMOD: u32 = R_ARM_TLS_DTPMOD32;
pub const REL_SYMBOLIC: u32 = R_ARM_ABS32;
pub const REL_JUMP_SLOT: u32 = R_ARM_JUMP_SLOT;
pub const REL_DTPOFF: u32 = R_ARM_TLS_DTPOFF32;
pub const REL_IRELATIVE: u32 = R_ARM_IRELATIVE;
pub const REL_COPY: u32 = R_ARM_COPY;
pub const REL_TPOFF: u32 = R_ARM_TLS_TPOFF32;

pub(crate) const DYLIB_OFFSET: usize = 1;
pub(crate) const RESOLVE_FUNCTION_OFFSET: usize = 2;

#[unsafe(naked)]
pub extern "C" fn dl_runtime_resolve() {
    core::arch::naked_asm!(
        "
    push {{r0, r1, r2, r3, r4}}
    ldr r0, [lr, #-4]
    add r1, lr, #4 
	sub r1, ip, r1
    lsr r1, r1, 2
    bl dl_fixup
    mov	ip, r0
    pop	{{r0, r1, r2, r3, r4, lr}}
    bx ip
	"
    )
}
