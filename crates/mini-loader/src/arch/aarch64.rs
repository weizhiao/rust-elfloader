use core::arch::global_asm;

global_asm!(
    "    
	.text
	.globl	_start
	.hidden	_start
	.type	_start,@function
_start:
	mov x29, #0
	mov x30, #0
	mov x0, sp
.weak _DYNAMIC
.hidden _DYNAMIC
	adrp x1, _DYNAMIC
	add x1, x1, #:lo12:_DYNAMIC
	and sp, x0, #-16
	b rust_main"
);

global_asm!(
    "	
	.text
	.align	4
	.globl	trampoline
	.type	trampoline,@function
trampoline:
	mov sp, x1
	mov x1, x0
	mov x0, #0
	br x1
	wfi"
);