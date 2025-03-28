use core::arch::global_asm;
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

global_asm!(
    "
    .text
    .globl dl_runtime_resolve
	.type dl_runtime_resolve, %function
	.align 16
dl_runtime_resolve:
"
);

pub(crate) fn prepare_lazy_bind(got: *mut usize, dylib: usize) {
    unsafe extern "C" {
        fn dl_runtime_resolve();
    }
    // 这是安全的，延迟绑定时库是存在的
    unsafe {
        got.add(1).write(dylib);
        got.add(2).write(dl_runtime_resolve as usize);
    }
    unimplemented!()
}
