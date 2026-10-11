/// Dynamic linker runtime resolver for RISC-V 64-bit PLT entries.
///
/// This function is called when a PLT entry needs to resolve a symbol address
/// at runtime. It saves the current register state including floating-point
/// registers, calls the dynamic linker resolution function, and then restores
/// the state before jumping to the resolved function.
///
/// The function preserves integer argument registers a0-a7 and return address ra
/// and optionally floating-point registers depending on the target calling ABI.
///
/// # Safety
/// This function uses naked assembly and must be called with the correct
/// stack layout set up by the PLT stub code.
#[unsafe(naked)]
pub(crate) extern "C" fn dl_runtime_resolve() {
    core::arch::naked_asm!(
        "
        // 保存整数参数寄存器
        // ra + a0-a7 占 72 字节，浮点参数紧随其后，栈帧向上对齐到 16 字节
        addi sp,sp,-{frame_size}
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
        #[cfg(riscv_float_abi = "d")]
        "
        .option push
        .option arch, +d
        fsd fa0,8*9(sp)
        fsd fa1,8*10(sp)
        fsd fa2,8*11(sp)
        fsd fa3,8*12(sp)
        fsd fa4,8*13(sp)
        fsd fa5,8*14(sp)
        fsd fa6,8*15(sp)
        fsd fa7,8*16(sp)
        .option pop
        ",
        #[cfg(riscv_float_abi = "f")]
        "
        .option push
        .option arch, +f
        fsw fa0,72+4*0(sp)
        fsw fa1,72+4*1(sp)
        fsw fa2,72+4*2(sp)
        fsw fa3,72+4*3(sp)
        fsw fa4,72+4*4(sp)
        fsw fa5,72+4*5(sp)
        fsw fa6,72+4*6(sp)
        fsw fa7,72+4*7(sp)
        .option pop
        ",
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
        #[cfg(riscv_float_abi = "d")]
        "
        .option push
        .option arch, +d
        fld fa0,8*9(sp)
        fld fa1,8*10(sp)
        fld fa2,8*11(sp)
        fld fa3,8*12(sp)
        fld fa4,8*13(sp)
        fld fa5,8*14(sp)
        fld fa6,8*15(sp)
        fld fa7,8*16(sp)
        .option pop
        ",
        #[cfg(riscv_float_abi = "f")]
        "
        .option push
        .option arch, +f
        flw fa0,72+4*0(sp)
        flw fa1,72+4*1(sp)
        flw fa2,72+4*2(sp)
        flw fa3,72+4*3(sp)
        flw fa4,72+4*4(sp)
        flw fa5,72+4*5(sp)
        flw fa6,72+4*6(sp)
        flw fa7,72+4*7(sp)
        .option pop
        ",
        "
        addi sp,sp,{frame_size}
        // 执行真正的函数
        jr t1
        ",
        sym crate::lazy::dl_fixup,
        frame_size = const if cfg!(riscv_float_abi = "d") {
            144 // 72 integer + 64 floating-point bytes, rounded up.
        } else if cfg!(riscv_float_abi = "f") {
            112 // 72 integer + 32 floating-point bytes, rounded up.
        } else {
            80 // 72 integer bytes, rounded up.
        },
    )
}
