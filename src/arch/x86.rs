use elf::abi::*;

pub const EM_ARCH: u16 = EM_386;
pub const TLS_DTV_OFFSET: usize = 0;

pub const REL_RELATIVE: u32 = R_X86_64_RELATIVE;
pub const REL_GOT: u32 = R_X86_64_GLOB_DAT;
pub const REL_DTPMOD: u32 = R_X86_64_DTPMOD64;
pub const REL_SYMBOLIC: u32 = R_X86_64_32;
pub const REL_JUMP_SLOT: u32 = R_X86_64_JUMP_SLOT;
pub const REL_DTPOFF: u32 = R_X86_64_DTPOFF32;
pub const REL_IRELATIVE: u32 = R_X86_64_IRELATIVE;
pub const REL_COPY: u32 = R_X86_64_COPY;
pub const REL_TPOFF: u32 = R_X86_64_TPOFF32;

#[cfg(feature = "lazy")]
core::arch::global_asm!(
    "
    .text
    .globl dl_runtime_resolve
	.type dl_runtime_resolve, @function
	.align 16
dl_runtime_resolve:
// 计算偏移
	mov ecx, [esp + 4]
	shr ecx, 3
	mov [esp + 4], ecx
// 与glibc不同,这里仍使用栈进行传参
	call dl_fixup
// 将函数的真正地址写回栈顶
	mov [esp], eax
// 清除plt代码压入栈中的东西,当执行完这条指令后栈顶保存的是plt代码对应的返回地址
	ret 4
"
);

#[cfg(feature = "lazy")]
#[inline]
pub(crate) fn prepare_lazy_bind(got: *mut usize, dylib: usize) {
    unsafe extern "C" {
        fn dl_runtime_resolve();
    }
    // 这是安全的，延迟绑定时库是存在的
    unsafe {
        got.add(1).write(dylib);
        got.add(2).write(dl_runtime_resolve as usize);
    }
}
