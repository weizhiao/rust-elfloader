//! Relocation of elf objects
use crate::{
    CoreComponent, Error, Result,
    arch::*,
    format::{CoreComponentInner, ElfCommonPart, Relocated},
    relocate_error,
    symbol::{SymbolInfo, SymbolTable},
};
use alloc::{boxed::Box, format, sync::Arc, vec::Vec};
use core::{
    any::Any,
    marker::PhantomData,
    num::NonZeroUsize,
    sync::atomic::{AtomicUsize, Ordering},
};
use elf::abi::*;

// lazy binding 时会先从这里寻找符号
pub(crate) static GLOBAL_SCOPE: AtomicUsize = AtomicUsize::new(0);

struct SymDef<'temp> {
    sym: &'temp ElfSymbol,
    base: usize,
}

impl<'temp> SymDef<'temp> {
    // 获取符号的真实地址(base + st_value)
    #[inline(always)]
    fn convert(self) -> *const () {
        if likely(self.sym.st_type() != STT_GNU_IFUNC) {
            (self.base + self.sym.st_value()) as _
        } else {
            // IFUNC会在运行时确定地址，这里使用的是ifunc的返回值
            let ifunc: fn() -> usize =
                unsafe { core::mem::transmute(self.base + self.sym.st_value()) };
            ifunc() as _
        }
    }
}

pub(crate) struct RelocateHelper<'core> {
    pub base: usize,
    pub symtab: &'core SymbolTable,
}

// 在此之前检查是否需要relocate
pub(crate) fn relocate_impl<'iter, 'find, 'lib, F, D>(
    common: ElfCommonPart,
    scope: Vec<RelocateHelper<'iter>>,
    pre_find: &'find F,
    deal_unknown: D,
    local_lazy_scope: Option<Box<dyn for<'a> Fn(&'a str) -> Option<*const ()> + 'static>>,
) -> Result<Relocated<'lib>>
where
    F: Fn(&str) -> Option<*const ()>,
    D: Fn(&ElfRela, &CoreComponent) -> core::result::Result<(), Box<dyn Any>>,
    'iter: 'lib,
    'find: 'lib,
{
    fn find_symdef<'iter, 'temp>(
        core: &'temp CoreComponent,
        libs: &[RelocateHelper<'iter>],
        dynsym: &'temp ElfSymbol,
        syminfo: &SymbolInfo,
    ) -> Option<SymDef<'temp>>
    where
        'iter: 'temp,
    {
        if unlikely(dynsym.is_local()) {
            Some(SymDef {
                sym: dynsym,
                base: core.base(),
            })
        } else {
            libs.iter().find_map(|lib| {
                lib.symtab.lookup_filter(&syminfo).map(|sym| SymDef {
                    sym,
                    base: lib.base,
                })
            })
        }
    }

    #[cold]
    fn reloc_error(
        r_type: usize,
        r_sym: usize,
        custom_err: Box<dyn Any>,
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

    /*
        A Represents the addend used to compute the value of the relocatable field.
        B Represents the base address at which a shared object has been loaded into memory during execution.
        S Represents the value of the symbol whose index resides in the relocation entry.
    */

    let symtab = common.symtab().unwrap();
    let relocation = &common.relocation;
    assert!(
        !(relocation.relative.len() > 0
            && relocation.relative[0].r_type() != REL_RELATIVE as usize)
    );
    relocation.relative.into_iter().for_each(|rela| {
        // B + A
        debug_assert!(rela.r_type() == REL_RELATIVE as usize);
        common.write_val(rela.r_offset(), common.base() + rela.r_addend());
    });

    for rela in relocation.dynrel {
        let r_type = rela.r_type() as _;
        let r_sym = rela.r_symbol();

        if unlikely(r_type == REL_RELATIVE) {
            common.write_val(rela.r_offset(), common.base() + rela.r_addend());
            continue;
        } else if unlikely(r_type == REL_NONE) {
            continue;
        }

        match r_type {
            // REL_GOT: S  REL_SYMBOLIC: S + A
            REL_GOT | REL_SYMBOLIC => {
                let (dynsym, syminfo) = symtab.symbol_idx(r_sym);
                if let Some(symbol) = pre_find(syminfo.name())
                    .or(find_symdef(&common, &scope, dynsym, &syminfo)
                        .map(|symdef| symdef.convert()))
                {
                    common.write_val(rela.r_offset(), symbol as usize);
                    continue;
                }
            }
            REL_DTPOFF => {
                let (dynsym, syminfo) = symtab.symbol_idx(r_sym);
                if let Some(symdef) = find_symdef(&common, &scope, dynsym, &syminfo) {
                    // offset in tls
                    let tls_val =
                        (symdef.sym.st_value() + rela.r_addend()).wrapping_sub(TLS_DTV_OFFSET);
                    common.write_val(rela.r_offset(), tls_val);
                    continue;
                }
            }
            REL_COPY => {
                let (dynsym, syminfo) = symtab.symbol_idx(r_sym);
                if let Some(symbol) = find_symdef(&common, &scope, dynsym, &syminfo) {
                    let len = symbol.sym.st_size();
                    let dest = unsafe {
                        core::slice::from_raw_parts_mut(
                            (common.base() + rela.r_offset()) as *mut u8,
                            len,
                        )
                    };
                    let src = unsafe {
                        core::slice::from_raw_parts(
                            (common.base() + symbol.sym.st_value()) as *const u8,
                            len,
                        )
                    };
                    dest.copy_from_slice(src);
                    continue;
                }
            }
            _ => {}
        }
        deal_unknown(&rela, &common)
            .map_err(|err| reloc_error(r_type as _, r_sym, err, &common))?;
    }

    // 开启lazy bind后会跳过plt相关的重定位
    if common.is_lazy() {
        for rela in relocation.pltrel {
            let r_type = rela.r_type() as u32;
            // S
            if likely(r_type == REL_JUMP_SLOT) {
                let ptr = (common.base() + rela.r_offset()) as *mut usize;
                // 即使是延迟加载也需要进行简单重定位，好让plt代码能够正常工作
                unsafe {
                    let origin_val = ptr.read();
                    let new_val = origin_val + common.base();
                    ptr.write(new_val);
                }
            } else if unlikely(r_type == REL_IRELATIVE) {
                let ifunc: fn() -> usize =
                    unsafe { core::mem::transmute(common.base() + rela.r_addend()) };
                common.write_val(rela.r_offset(), ifunc());
            } else {
                unreachable!()
            }
        }
        if !relocation.pltrel.is_empty() {
            prepare_lazy_bind(
                common.got.unwrap().as_ptr(),
                Arc::as_ptr(&common.inner) as usize,
            );
        }
        assert!(
            relocation.pltrel.is_empty()
                || local_lazy_scope.is_some()
                || GLOBAL_SCOPE.load(Ordering::Relaxed) != 0,
            "neither local lazy scope nor global scope is set"
        );
        common.set_lazy_scope(local_lazy_scope);
    } else {
        for rela in relocation.pltrel {
            let r_type = rela.r_type() as u32;
            let r_sym = rela.r_symbol();
            // S
            // 对于.rela.plt来说通常只有这两种重定位类型
            if likely(r_type == REL_JUMP_SLOT) {
                let (dynsym, syminfo) = symtab.symbol_idx(r_sym);
                if let Some(symbol) = pre_find(syminfo.name())
                    .or(find_symdef(&common, &scope, dynsym, &syminfo)
                        .map(|symdef| symdef.convert()))
                {
                    common.write_val(rela.r_offset(), symbol as usize);
                    continue;
                }
            } else if unlikely(r_type == REL_IRELATIVE) {
                let ifunc: fn() -> usize =
                    unsafe { core::mem::transmute(common.base() + rela.r_addend()) };
                common.write_val(rela.r_offset(), ifunc());
                continue;
            }
            deal_unknown(&rela, &common)
                .map_err(|err| reloc_error(r_type as _, r_sym, err, &common))?;
        }
        if let Some(relro) = common.relro {
            relro.relro()?;
        }
    }

    common.init.call_init();
    common.core.set_init();
    Ok(Relocated {
        core: common.core,
        _marker: PhantomData,
    })
}

impl CoreComponent {
    #[inline(always)]
    fn write_val(&self, offset: usize, val: usize) {
        unsafe {
            let rel_addr = (self.base() + offset) as *mut usize;
            rel_addr.write(val)
        };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn dl_fixup(dylib: &CoreComponentInner, rela_idx: usize) -> usize {
    let rela = unsafe { &*dylib.pltrel.unwrap().add(rela_idx).as_ptr() };
    let r_type = rela.r_type();
    let r_sym = rela.r_symbol();
    assert!(r_type == REL_JUMP_SLOT as usize && r_sym != 0);
    let (_, syminfo) = dylib.symbols.as_ref().unwrap().symbol_idx(r_sym);
    let scope = GLOBAL_SCOPE.load(core::sync::atomic::Ordering::Acquire);
    let symbol = if scope == 0 {
        dylib.lazy_scope.as_ref().unwrap()(syminfo.name())
    } else {
        unsafe { core::mem::transmute::<_, fn(&str) -> Option<*const ()>>(scope)(syminfo.name()) }
            .or(dylib.lazy_scope.as_ref().unwrap()(syminfo.name()))
    }
    .expect("lazy bind fail") as usize;
    let ptr = (dylib.segments.base() + rela.r_offset()) as *mut usize;
    unsafe { ptr.write(symbol) };
    symbol
}

#[derive(Default)]
pub(crate) struct ElfRelocation {
    // REL_RELATIVE
    relative: &'static [ElfRela],
    // plt
    pltrel: &'static [ElfRela],
    // others in dyn
    dynrel: &'static [ElfRela],
}

impl ElfRelocation {
    #[inline]
    pub(crate) fn new(
        pltrel: Option<&'static [ElfRela]>,
        dynrel: Option<&'static [ElfRela]>,
        rela_count: Option<NonZeroUsize>,
    ) -> Self {
        // nrelative记录着REL_RELATIVE重定位类型的个数
        let nrelative = rela_count.map(|v| v.get()).unwrap_or(0);
        let old_dynrel = dynrel.unwrap_or(&[]);
        let relative = &old_dynrel[..nrelative];
        let temp_dynrel = &old_dynrel[nrelative..];
        let pltrel = pltrel.unwrap_or(&[]);
        let dynrel = if unsafe {
            old_dynrel.as_ptr().add(old_dynrel.len()) == pltrel.as_ptr().add(pltrel.len())
        } {
            &temp_dynrel[..temp_dynrel.len() - pltrel.len()]
        } else {
            temp_dynrel
        };
        Self {
            relative,
            pltrel,
            dynrel,
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.relative.is_empty() && self.dynrel.is_empty() && self.pltrel.is_empty()
    }
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
