use core::arch::global_asm;
use elf::abi::*;

pub const EM_ARCH: u16 = EM_AARCH64;
pub const TLS_DTV_OFFSET: usize = 0;

pub const REL_RELATIVE: u32 = R_AARCH64_RELATIVE;
pub const REL_GOT: u32 = R_AARCH64_GLOB_DAT;
pub const REL_DTPMOD: u32 = R_AARCH64_TLS_DTPMOD;
pub const REL_SYMBOLIC: u32 = R_AARCH64_ABS64;
pub const REL_JUMP_SLOT: u32 = R_AARCH64_JUMP_SLOT;
pub const REL_DTPOFF: u32 = R_AARCH64_TLS_DTPREL;

global_asm!(
    "
    .text
    .globl dl_runtime_resolve
	.type dl_runtime_resolve, @function
	.align 16
dl_runtime_resolve:
// 保存参数寄存器
    sub sp,sp,8*8
    stp x0,x1,[sp,16*0]
    stp x2,x3,[sp,16*1]
    stp x4,x5,[sp,16*2]
    stp x6,x7,[sp,16*3]
// 读取需要的参数
    ldr x0,[x16,-8]
    ldr x1,[sp,16*4]
    sub x1,x1,x16
    sub x1,x1,8
    lsr x1,x1,3
    bl	dl_fixup
    mov x16,x0
// 恢复参数寄存器
    ldp x0,x1,[sp,16*0]
    ldp x2,x3,[sp,16*1]
    ldp x4,x5,[sp,16*2]
    ldp x6,x7,[sp,16*3]
    ldr x30,[sp,16*4+8]
// 这里要将plt代码压入栈中的东西也弹出去
    add sp,sp,8*10
    br x16
"
);

pub(crate) fn prepare_lazy_bind(got: *mut usize, dylib: usize) {
    extern "C" {
        fn dl_runtime_resolve();
    }
    // 这是安全的，延迟绑定时库是存在的
    unsafe {
        got.add(1).write(dylib);
        got.add(2).write(dl_runtime_resolve as usize);
    }
}
