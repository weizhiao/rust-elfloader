use crate::{
    arch::ElfRelType,
    relocation::{find_symbol_addr, static_link::StaticReloc, write_val},
};
use elf::abi::*;

pub const EM_ARCH: u16 = EM_X86_64;
pub const TLS_DTV_OFFSET: usize = 0;

pub const REL_RELATIVE: u32 = R_X86_64_RELATIVE;
pub const REL_GOT: u32 = R_X86_64_GLOB_DAT;
pub const REL_DTPMOD: u32 = R_X86_64_DTPMOD64;
pub const REL_SYMBOLIC: u32 = R_X86_64_64;
pub const REL_JUMP_SLOT: u32 = R_X86_64_JUMP_SLOT;
pub const REL_DTPOFF: u32 = R_X86_64_DTPOFF64;
pub const REL_IRELATIVE: u32 = R_X86_64_IRELATIVE;
pub const REL_COPY: u32 = R_X86_64_COPY;
pub const REL_TPOFF: u32 = R_X86_64_TPOFF64;

pub(crate) const DYLIB_OFFSET: usize = 1;
pub(crate) const RESOLVE_FUNCTION_OFFSET: usize = 2;

#[unsafe(naked)]
pub extern "C" fn dl_runtime_resolve() {
    core::arch::naked_asm!(
        "
// 保存参数寄存器,这里多使用了8字节栈是为了栈的16字节对齐
    sub rsp,8*7
    mov [rsp+8*0],rdi
    mov [rsp+8*1],rsi
    mov [rsp+8*2],rdx
    mov [rsp+8*3],rcx
    mov [rsp+8*4],r8
    mov [rsp+8*5],r9
// 这两个是plt代码压入栈的
    mov rdi,[rsp+8*7]
    mov rsi,[rsp+8*8]
// 调用重定位函数
    call dl_fixup
// 恢复参数寄存器
    mov rdi,[rsp+8*0]
    mov rsi,[rsp+8*1]
    mov rdx,[rsp+8*2]
    mov rcx,[rsp+8*3]
    mov r8,[rsp+8*4]
    mov r9,[rsp+8*5]
// 需要把plt代码压入栈中的东西也弹出去
    add rsp,7*8+2*8
// 执行真正的函数
    jmp rax
	"
    )
}

pub(crate) struct X86_64Relocator;

impl StaticReloc for X86_64Relocator {
    fn relocate<F>(
        core: &crate::CoreComponent,
        rel_type: &ElfRelType,
        l: usize,
        target_base: usize,
        scope: &[&crate::format::Relocated],
        pre_find: &F,
    ) -> crate::Result<()>
    where
        F: Fn(&str) -> Option<*const ()>,
    {
        let symtab = core.symtab().unwrap();
        let r_sym = rel_type.r_symbol();
        let base = core.base();
        let append = rel_type.r_addend(base);
        let offset = target_base - base + rel_type.r_offset();
        let p = target_base + rel_type.r_offset();
        match rel_type.r_type() as _ {
            R_X86_64_PC32 | R_X86_64_PLT32 => {
                if let Some(sym) = find_symbol_addr(pre_find, core, symtab, scope, r_sym) {
                    write_val(
                        base,
                        offset,
                        (sym.wrapping_add_signed(append).wrapping_sub(p)) as u32,
                    );
                    return Ok(());
                }
            }
            _ => {}
        }
        panic!();
        Ok(())
    }
}
