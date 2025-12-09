#![no_std]
#![crate_type = "cdylib"]
#![allow(unused)]
#![allow(bad_asm_style)]

use core::arch::global_asm;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

extern "C" {
    fn external_func();
    static external_var: i32;
}

global_asm!(
    ".intel_syntax noprefix",
    ".section .text",
    ".globl asm_test_func",
    "asm_test_func:",
    "push rbx",
    // R_X86_64_PLT32
    "call external_func@PLT",
    "mov ebx, eax", // ebx = 100
    // R_X86_64_GOTPCREL
    "mov rax, [rip + external_var@GOTPCREL]",
    "mov ecx, [rax]",
    "add ebx, ecx", // ebx = 100 + 200 = 300
    // R_X86_64_PC32
    "lea rax, [rip + local_var]",
    "mov edx, [rax]",
    "add ebx, edx", // ebx = 300 + 42 = 342
    // R_X86_64_32S (or 32)
    "mov rax, offset external_var_32",
    "add rax, rbx", // rax = 0x1000 + 342 = 0x1156
    "pop rbx",
    "ret",
    ".att_syntax",
    ".section .data",
    ".globl local_var",
    "local_var:",
    ".long 42",
    ".section .data.relocs",
    // R_X86_64_64
    ".quad external_func",
    // R_X86_64_32
    ".long external_var_32",
);
