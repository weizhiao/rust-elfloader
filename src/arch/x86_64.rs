use crate::{
    arch::ElfRelType,
    relocation::{RelocValue, find_symbol_addr, reloc_error, static_link::StaticReloc, SymbolLookup},
    segment::shdr::{GotEntry, PltEntry, PltGotSection},
};
use alloc::boxed::Box;
use elf::abi::*;

#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

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
pub(crate) const PLT_ENTRY_SIZE: usize = 16;

pub(crate) const PLT_ENTRY: [u8; PLT_ENTRY_SIZE] = [
    0xf3, 0x0f, 0x1e, 0xfa, // endbr64
    0xff, 0x25, 0, 0, 0, 0, // jmp *GOTPLT+idx(%rip)
    0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, // (padding)
];

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
    fn relocate<S: SymbolLookup + ?Sized>(
        core: &crate::CoreComponent,
        rel_type: &ElfRelType,
        pltgot: &mut PltGotSection,
        scope: &[crate::format::Relocated],
        pre_find: &S,
    ) -> crate::Result<()> {
        let symtab = core.symtab().unwrap();
        let r_sym = rel_type.r_symbol();
        let r_type = rel_type.r_type();
        let base = core.base();
        let segments = core.segments();
        let append = rel_type.r_addend(base);
        let offset = rel_type.r_offset();
        let p = base + rel_type.r_offset();
        let find_symbol = |r_sym: usize| find_symbol_addr(pre_find, core, symtab, scope, r_sym);
        let boxed_error = || reloc_error(r_type, r_sym, Box::new(()), core);
        match r_type as _ {
            R_X86_64_64 => {
                let Some(sym) = find_symbol(r_sym) else {
                    return Err(boxed_error());
                };
                segments.write(offset, sym + append);
            }
            R_X86_64_PC32 => {
                let Some(sym) = find_symbol(r_sym) else {
                    return Err(boxed_error());
                };
                let val: RelocValue<i32> = (sym + append - p).try_into().unwrap();
                segments.write(offset, val);
            }
            R_X86_64_PLT32 => {
                let Some(sym) = find_symbol(r_sym) else {
                    return Err(boxed_error());
                };
                let val: RelocValue<i32> = if let Ok(val) = (sym + append - p).try_into() {
                    val
                } else {
                    let plt_entry = pltgot.add_plt_entry(r_sym);
                    let plt_entry_addr = match plt_entry {
                        PltEntry::Occupied(plt_entry_addr) => plt_entry_addr,
                        PltEntry::Vacant { plt, mut got } => {
                            let plt_entry_addr = plt.as_ptr() as usize;
                            got.update(sym.into());
                            let call_offset = got.get_addr() - plt_entry_addr - 10;
                            let call_offset_val: RelocValue<i32> = call_offset.try_into().unwrap();
                            plt[6..10].copy_from_slice(&call_offset_val.0.to_ne_bytes());
                            RelocValue::new(plt_entry_addr)
                        }
                    };
                    (plt_entry_addr + append - p).try_into().unwrap()
                };
                segments.write(offset, val);
            }
            R_X86_64_GOTPCREL => {
                let Some(sym) = find_symbol(r_sym) else {
                    return Err(boxed_error());
                };
                let got_entry = pltgot.add_got_entry(r_sym);
                let got_entry_addr = match got_entry {
                    GotEntry::Occupied(got_entry_addr) => got_entry_addr,
                    GotEntry::Vacant(mut got) => {
                        got.update(sym);
                        got.get_addr()
                    }
                };
                let val: RelocValue<i32> = (got_entry_addr + append - p).try_into().unwrap();
                segments.write(offset, val);
            }
            R_X86_64_32 => {
                let Some(sym) = find_symbol(r_sym) else {
                    return Err(boxed_error());
                };
                let val: RelocValue<u32> = (sym + append).try_into().unwrap();
                segments.write(offset, val);
            }
            R_X86_64_32S => {
                let Some(sym) = find_symbol(r_sym) else {
                    return Err(boxed_error());
                };
                let val: RelocValue<i32> = (sym + append).try_into().unwrap();
                segments.write(offset, val);
            }
            _ => {
                return Err(boxed_error());
            }
        }
        Ok(())
    }

    fn needs_got(rel_type: u32) -> bool {
        rel_type == R_X86_64_GOTPCREL
    }

    fn needs_plt(rel_type: u32) -> bool {
        rel_type == R_X86_64_PLT32
    }
}
