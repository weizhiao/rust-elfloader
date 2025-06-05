use core::arch::global_asm;

global_asm!(
    "
    .section .text.entry
    .globl _start
    .hidden _start
    .type _start,@function
_start:
	lla   gp, __global_pointer$
    mv      a0, sp
    .weak   _DYNAMIC
    .hidden _DYNAMIC
    lla      a1, _DYNAMIC
    // 调用 rust_main 函数
    tail    rust_main
"
);

global_asm!(
    "
    .section .text.trampoline
    .globl  trampoline
	.align	4
    .type   trampoline,@function
trampoline:
    mv      sp, a1
	mv      t0, a0
	// rtld_fini
	li      a0, 0
    jr      t0
	ebreak
"
);
