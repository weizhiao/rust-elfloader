use elf::abi::*;

pub const EM_ARCH: u16 = EM_RISCV;
/* Dynamic thread vector pointers point 0x800 past the start of each
TLS block.  */
pub const TLS_DTV_OFFSET: usize = 0x800;

pub const REL_RELATIVE: u32 = R_RISCV_RELATIVE;
pub const REL_GOT: u32 = R_RISCV_64;
pub const REL_DTPMOD: u32 = R_RISCV_TLS_DTPMOD64;
pub const REL_SYMBOLIC: u32 = R_RISCV_64;
pub const REL_JUMP_SLOT: u32 = R_RISCV_JUMP_SLOT;
pub const REL_DTPOFF: u32 = R_RISCV_TLS_DTPREL64;
pub const REL_IRELATIVE: u32 = R_RISCV_IRELATIVE;
pub const REL_COPY: u32 = R_RISCV_COPY;
pub const REL_TPOFF: u32 = R_RISCV_TLS_TPREL64;

pub(crate) const DYLIB_OFFSET: usize = 1;
pub(crate) const RESOLVE_FUNCTION_OFFSET: usize = 0;

macro_rules! riscv64_dl_runtime_resolve {
    ($save_fprs:expr, $restore_fprs:expr) => {
        #[unsafe(naked)]
        pub(crate) extern "C" fn dl_runtime_resolve() {
            core::arch::naked_asm!(
                "
                // 保存整数参数寄存器
                // 18 * 8 = 144 bytes, 保持 16 字节对齐
                addi sp,sp,-18*8
                sd ra,8*0(sp)
                sd a0,8*1(sp)
                sd a1,8*2(sp)
                sd a2,8*3(sp)
                sd a3,8*4(sp)
                sd a4,8*5(sp)
                sd a5,8*6(sp)
                sd a6,8*7(sp)
                sd a7,8*8(sp)
                ",
                $save_fprs,
                "
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
                ",
                $restore_fprs,
                "
                addi sp,sp,18*8
                // 执行真正的函数
                jr t1
                ",
                sym crate::relocation::dl_fixup,
            )
        }
    };
}

#[cfg(target_feature = "d")]
riscv64_dl_runtime_resolve!(
    "
    fsd fa0,8*9(sp)
    fsd fa1,8*10(sp)
    fsd fa2,8*11(sp)
    fsd fa3,8*12(sp)
    fsd fa4,8*13(sp)
    fsd fa5,8*14(sp)
    fsd fa6,8*15(sp)
    fsd fa7,8*16(sp)
    ",
    "
    fld fa0,8*9(sp)
    fld fa1,8*10(sp)
    fld fa2,8*11(sp)
    fld fa3,8*12(sp)
    fld fa4,8*13(sp)
    fld fa5,8*14(sp)
    fld fa6,8*15(sp)
    fld fa7,8*16(sp)
    "
);

#[cfg(all(target_feature = "f", not(target_feature = "d")))]
riscv64_dl_runtime_resolve!(
    "
    fsw fa0,8*9(sp)
    fsw fa1,8*10(sp)
    fsw fa2,8*11(sp)
    fsw fa3,8*12(sp)
    fsw fa4,8*13(sp)
    fsw fa5,8*14(sp)
    fsw fa6,8*15(sp)
    fsw fa7,8*16(sp)
    ",
    "
    flw fa0,8*9(sp)
    flw fa1,8*10(sp)
    flw fa2,8*11(sp)
    flw fa3,8*12(sp)
    flw fa4,8*13(sp)
    flw fa5,8*14(sp)
    flw fa6,8*15(sp)
    flw fa7,8*16(sp)
    "
);

#[cfg(not(target_feature = "f"))]
riscv64_dl_runtime_resolve!("", "");

/// Map riscv64 relocation types to human readable names
pub(crate) fn rel_type_to_str(r_type: usize) -> &'static str {
    match r_type as u32 {
        R_RISCV_NONE => "R_RISCV_NONE",
        R_RISCV_64 => "R_RISCV_64",
        R_RISCV_RELATIVE => "R_RISCV_RELATIVE",
        R_RISCV_COPY => "R_RISCV_COPY",
        R_RISCV_JUMP_SLOT => "R_RISCV_JUMP_SLOT",
        R_RISCV_IRELATIVE => "R_RISCV_IRELATIVE",
        _ => "UNKNOWN",
    }
}
