use crate::{
    arch::ElfRelType,
    relocation::{RelocValue, StaticReloc, SymbolLookup, find_symbol_addr, reloc_error},
    segment::shdr::{GotEntry, PltEntry, PltGotSection},
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
    // Save caller-saved registers
    push rdi
    push rsi
    push rdx
    push rcx
    push r8
    push r9
    push r10
    push r11

    // Save xmm registers (arguments can be passed in xmm0-xmm7)
    // We need 128 bytes for xmm0-xmm7 + 8 bytes padding to align stack to 16 bytes
    sub rsp, 136
    movdqu [rsp + 0], xmm0
    movdqu [rsp + 16], xmm1
    movdqu [rsp + 32], xmm2
    movdqu [rsp + 48], xmm3
    movdqu [rsp + 64], xmm4
    movdqu [rsp + 80], xmm5
    movdqu [rsp + 96], xmm6
    movdqu [rsp + 112], xmm7

    // Arguments for dl_fixup(link_map, reloc_idx)
    // link_map was pushed by PLT0, reloc_idx was pushed by PLT entry
    // Stack layout now:
    // [rsp + 0..127]  : xmm0-xmm7
    // [rsp + 128..135]: padding
    // [rsp + 136..199]: r11, r10, r9, r8, rcx, rdx, rsi, rdi (8 * 8 = 64)
    // [rsp + 200]     : link_map
    // [rsp + 208]     : reloc_idx
    // [rsp + 216]     : return address to caller
    mov rdi, [rsp + 200]
    mov rsi, [rsp + 208]

    // Call the resolver
    call {0}

    // Restore xmm registers
    movdqu xmm0, [rsp + 0]
    movdqu xmm1, [rsp + 16]
    movdqu xmm2, [rsp + 32]
    movdqu xmm3, [rsp + 48]
    movdqu xmm4, [rsp + 64]
    movdqu xmm5, [rsp + 80]
    movdqu xmm6, [rsp + 96]
    movdqu xmm7, [rsp + 112]
    add rsp, 136

    // Restore caller-saved registers
    pop r11
    pop r10
    pop r9
    pop r8
    pop rcx
    pop rdx
    pop rsi
    pop rdi

    // Clean up link_map and reloc_idx from stack
    add rsp, 16

    // Jump to the resolved function
    jmp rax
    ",
        sym crate::relocation::dl_fixup,
    )
}

pub(crate) struct X86_64Relocator;

/// Map x86_64 relocation type value to human readable name.
pub fn rel_type_to_str(r_type: usize) -> &'static str {
    match r_type as u32 {
        R_X86_64_NONE => "R_X86_64_NONE",
        R_X86_64_64 => "R_X86_64_64",
        R_X86_64_PC32 => "R_X86_64_PC32",
        R_X86_64_GOT32 => "R_X86_64_GOT32",
        R_X86_64_PLT32 => "R_X86_64_PLT32",
        R_X86_64_COPY => "R_X86_64_COPY",
        R_X86_64_GLOB_DAT => "R_X86_64_GLOB_DAT",
        R_X86_64_JUMP_SLOT => "R_X86_64_JUMP_SLOT",
        R_X86_64_RELATIVE => "R_X86_64_RELATIVE",
        R_X86_64_GOTPCREL => "R_X86_64_GOTPCREL",
        R_X86_64_32 => "R_X86_64_32",
        R_X86_64_32S => "R_X86_64_32S",
        R_X86_64_IRELATIVE => "R_X86_64_IRELATIVE",
        _ => "UNKNOWN",
    }
}

impl StaticReloc for X86_64Relocator {
    fn relocate<PreS, PostS>(
        core: &crate::format::ElfModule<()>,
        rel_type: &ElfRelType,
        pltgot: &mut PltGotSection,
        scope: &[crate::format::LoadedModule<()>],
        pre_find: &PreS,
        post_find: &PostS,
    ) -> crate::Result<()>
    where
        PreS: SymbolLookup + ?Sized,
        PostS: SymbolLookup + ?Sized,
    {
        let symtab = core.symtab();
        let r_sym = rel_type.r_symbol();
        let r_type = rel_type.r_type();
        let base = core.base();
        let segments = core.segments();
        let append = rel_type.r_addend(base);
        let offset = rel_type.r_offset();
        let p = base + rel_type.r_offset();
        let find_symbol = |r_sym: usize| {
            find_symbol_addr(pre_find, post_find, core, symtab, scope, r_sym).map(|(val, _)| val)
        };
        let boxed_error = || reloc_error(rel_type, "unknown symbol", core);
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
                let val: RelocValue<i32> = (sym + append - p).try_into().map_err(|_| {
                    reloc_error(
                        rel_type,
                        "out of range integral type conversion attempted",
                        core,
                    )
                })?;
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
                let val: RelocValue<u32> = (sym + append).try_into().map_err(|_| {
                    reloc_error(
                        rel_type,
                        "out of range integral type conversion attempted",
                        core,
                    )
                })?;
                segments.write(offset, val);
            }
            R_X86_64_32S => {
                let Some(sym) = find_symbol(r_sym) else {
                    return Err(boxed_error());
                };
                let val: RelocValue<i32> = (sym + append).try_into().map_err(|_| {
                    reloc_error(
                        rel_type,
                        "out of range integral type conversion attempted",
                        core,
                    )
                })?;
                segments.write(offset, val);
            }
            _ => {
                return Err(boxed_error());
            }
        }
        Ok(())
    }

    fn needs_got(rel_type: u32) -> bool {
        matches!(rel_type, R_X86_64_GOTPCREL | R_X86_64_PLT32)
    }

    fn needs_plt(rel_type: u32) -> bool {
        rel_type == R_X86_64_PLT32
    }
}
