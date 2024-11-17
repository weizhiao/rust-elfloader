	.text
	.align	4
	.globl	trampoline
	.type	trampoline,@function
trampoline:
	mov	rsp, rsi
	jmp	rdi
	/* Should not reach. */
	hlt

