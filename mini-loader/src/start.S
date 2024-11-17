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
	hlt