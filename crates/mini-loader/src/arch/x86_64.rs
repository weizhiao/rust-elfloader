use core::arch::global_asm;

global_asm!(
    "    
	.text
	.globl	_start
	.hidden	_start
	.type	_start,@function
_start:
	mov	rdi, rsp
.weak _DYNAMIC
.hidden _DYNAMIC
	lea rsi, [rip + _DYNAMIC]
	call rust_main
	hlt"
);

global_asm!(
    "	
	.text
	.align	4
	.globl	trampoline
	.type	trampoline,@function
trampoline:
	xor rdx, rdx
	mov	rsp, rsi
	jmp	rdi
	/* Should not reach. */
	hlt"
);