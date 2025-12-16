use elf::abi::*;

pub const EM_ARCH: u16 = EM_RISCV;
/* Dynamic thread vector pointers point 0x800 past the start of each
TLS block.  */
pub const TLS_DTV_OFFSET: usize = 0x800;

pub const REL_RELATIVE: u32 = R_RISCV_RELATIVE;
// RISCV does not have this
pub const REL_GOT: u32 = u32::MAX;
pub const REL_DTPMOD: u32 = R_RISCV_TLS_DTPMOD64;
pub const REL_SYMBOLIC: u32 = R_RISCV_64;
pub const REL_JUMP_SLOT: u32 = R_RISCV_JUMP_SLOT;
pub const REL_DTPOFF: u32 = R_RISCV_TLS_DTPREL64;
pub const REL_IRELATIVE: u32 = R_RISCV_IRELATIVE;
pub const REL_COPY: u32 = R_RISCV_COPY;
pub const REL_TPOFF: u32 = R_RISCV_TLS_TPREL64;

pub(crate) const DYLIB_OFFSET: usize = 1;
pub(crate) const RESOLVE_FUNCTION_OFFSET: usize = 0;

#[unsafe(naked)]
pub extern "C" fn dl_runtime_resolve() {
    core::arch::naked_asm!(
        "
// 保存参数寄存器,因为dl_fixup不会使用浮点参数寄存器,因此不需要保存
    addi sp,sp,-9*8
    sd ra,8*0(sp)
    sd a0,8*1(sp)
    sd a1,8*2(sp)
    sd a2,8*3(sp)
    sd a3,8*4(sp)
    sd a4,8*5(sp)
    sd a5,8*6(sp)
    sd a6,8*7(sp)
    sd a7,8*8(sp)
// 这两个是plt代码设置的
    mv a0,t0
    srli a1,t1,3
// 调用重定位函数
    call {0}
// 恢复参数寄存器
    mv t1,a0
    ld ra,8*0(sp)
    ld a0,8*1(sp)
    ld a1,8*2(sp)
    ld a2,8*3(sp)
    ld a3,8*4(sp)
    ld a4,8*5(sp)
    ld a5,8*6(sp)
    ld a6,8*7(sp)
    ld a7,8*8(sp)
    addi sp,sp,8*9
// 执行真正的函数
    jr t1
	",
        sym crate::relocation::dynamic_link::dl_fixup,
    )
}

/// Map riscv64 relocation types to human readable names
pub fn rel_type_to_str(r_type: usize) -> &'static str {
    match r_type as u32 {
        R_RISCV_NONE => "R_RISCV_NONE",
        R_RISCV_64 => "R_RISCV_64",
        R_RISCV_RELATIVE => "R_RISCV_RELATIVE",
        R_RISCV_COPY => "R_RISCV_COPY",
        R_RISCV_JUMP_SLOT => "R_RISCV_JUMP_SLOT",
        R_RISCV_IRELATIVE => "R_RISCV_IRELATIVE",
        _ => "R_RISCV_UNKNOWN",
    }
}
