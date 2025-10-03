use crate::{
    CoreComponent, Error,
    arch::ElfSymbol,
    format::Relocated,
    relocate_error,
    symbol::{SymbolInfo, SymbolTable},
};
use alloc::{boxed::Box, format};
use core::{any::Any, ptr::null};
use elf::abi::STT_GNU_IFUNC;

pub(crate) mod dynamic_link;
pub(crate) mod static_link;

pub struct SymDef<'lib> {
    pub sym: Option<&'lib ElfSymbol>,
    pub lib: &'lib CoreComponent,
}

impl<'temp> SymDef<'temp> {
    // 获取符号的真实地址(base + st_value)
    #[inline(always)]
    pub fn convert(self) -> *const () {
        if likely(self.sym.is_some()) {
            let base = self.lib.base();
            let sym = unsafe { self.sym.unwrap_unchecked() };
            if likely(sym.st_type() != STT_GNU_IFUNC) {
                (base + sym.st_value()) as _
            } else {
                // IFUNC会在运行时确定地址，这里使用的是ifunc的返回值
                let ifunc: fn() -> usize = unsafe { core::mem::transmute(base + sym.st_value()) };
                ifunc() as _
            }
        } else {
            // 未定义的弱符号返回null
            null()
        }
    }
}

#[cold]
pub(crate) fn reloc_error(
    r_type: usize,
    r_sym: usize,
    custom_err: Box<dyn Any + Send + Sync>,
    lib: &CoreComponent,
) -> Error {
    if r_sym == 0 {
        relocate_error(
            format!(
                "file: {}, relocation type: {}, no symbol",
                lib.shortname(),
                r_type,
            ),
            custom_err,
        )
    } else {
        relocate_error(
            format!(
                "file: {}, relocation type: {}, symbol name: {}",
                lib.shortname(),
                r_type,
                lib.symtab().unwrap().symbol_idx(r_sym).1.name(),
            ),
            custom_err,
        )
    }
}

fn find_weak<'lib>(lib: &'lib CoreComponent, dynsym: &'lib ElfSymbol) -> Option<SymDef<'lib>> {
    // 弱符号 + WEAK 用 0 填充rela offset
    if dynsym.is_weak() && dynsym.is_undef() {
        assert!(dynsym.st_value() == 0);
        Some(SymDef { sym: None, lib })
    } else if dynsym.st_value() != 0 {
        Some(SymDef {
            sym: Some(dynsym),
            lib,
        })
    } else {
        None
    }
}

pub fn find_symdef<'iter, 'lib>(
    core: &'lib CoreComponent,
    libs: &[&'iter Relocated],
    r_sym: usize,
) -> Option<SymDef<'lib>>
where
    'iter: 'lib,
{
    let symbol = core.symtab().unwrap();
    let (sym, syminfo) = symbol.symbol_idx(r_sym);
    find_symdef_impl(core, libs, sym, &syminfo)
}

#[inline]
pub(crate) fn find_symbol_addr<F>(
    pre_find: F,
    core: &CoreComponent,
    symtab: &SymbolTable,
    scope: &[&Relocated],
    r_sym: usize,
) -> Option<usize>
where
    F: Fn(&str) -> Option<*const ()>,
{
    let (dynsym, syminfo) = symtab.symbol_idx(r_sym);
    if let Some(addr) = pre_find(syminfo.name()) {
        #[cfg(feature = "log")]
        log::trace!(
            "binding file [{}] to [pre_find]: symbol [{}]",
            core.name(),
            syminfo.name()
        );
        return Some(addr as usize);
    }
    find_symdef_impl(core, scope, dynsym, &syminfo)
        .map(|symdef| symdef.convert())
        .map(|addr| addr as usize)
}

fn find_symdef_impl<'iter, 'lib>(
    core: &'lib CoreComponent,
    libs: &[&'iter Relocated],
    sym: &'lib ElfSymbol,
    syminfo: &SymbolInfo,
) -> Option<SymDef<'lib>>
where
    'iter: 'lib,
{
    if unlikely(sym.is_local()) {
        Some(SymDef {
            sym: Some(sym),
            lib: core,
        })
    } else {
        let mut precompute = syminfo.precompute();
        libs.iter()
            .find_map(|lib| {
                lib.symtab()
                    .lookup_filter(syminfo, &mut precompute)
                    .map(|sym| {
                        #[cfg(feature = "log")]
                        log::trace!(
                            "binding file [{}] to [{}]: symbol [{}]",
                            core.name(),
                            lib.name(),
                            syminfo.name()
                        );
                        SymDef {
                            sym: Some(sym),
                            lib: &lib,
                        }
                    })
            })
            .or_else(|| find_weak(core, sym))
    }
}

#[inline(always)]
pub(crate) fn write_val<T>(base: usize, offset: usize, val: T) {
    unsafe {
        let rel_addr = (base + offset) as *mut T;
        rel_addr.write(val);
    };
}

#[inline]
#[cold]
fn cold() {}

#[inline]
fn likely(b: bool) -> bool {
    if !b {
        cold()
    }
    b
}

#[inline]
fn unlikely(b: bool) -> bool {
    if b {
        cold()
    }
    b
}
