//! Relocation of elf objects
use crate::{
    CoreComponent, Error, RelocatedDylib, Result,
    arch::*,
    format::{CoreComponentInner, ElfCommonPart, Relocated},
    relocate_error,
    symbol::SymbolInfo,
};
use alloc::{boxed::Box, format, sync::Arc, vec::Vec};
use core::{
    any::Any,
    marker::PhantomData,
    num::NonZeroUsize,
    ptr::{null, null_mut},
    sync::atomic::{AtomicUsize, Ordering},
};
use elf::abi::*;

// lazy binding 时会先从这里寻找符号
pub(crate) static GLOBAL_SCOPE: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct SymDef<'temp> {
    pub(crate) sym: Option<&'temp ElfSymbol>,
    pub(crate) base: usize,
}

impl<'temp> SymDef<'temp> {
    // 获取符号的真实地址(base + st_value)
    #[inline(always)]
    pub(crate) fn convert(self) -> *const () {
        if likely(self.sym.is_some()) {
            let sym = unsafe { self.sym.unwrap_unchecked() };
            if likely(sym.st_type() != STT_GNU_IFUNC) {
                (self.base + sym.st_value()) as _
            } else {
                // IFUNC会在运行时确定地址，这里使用的是ifunc的返回值
                let ifunc: fn() -> usize =
                    unsafe { core::mem::transmute(self.base + sym.st_value()) };
                ifunc() as _
            }
        } else {
            // 未定义的弱符号返回null
            null()
        }
    }
}

pub(crate) type LazyScope<'lib> = Arc<dyn for<'a> Fn(&'a str) -> Option<*const ()> + 'lib>;

pub(crate) type DealUnknown = dyn Fn(
    &ElfRelType,
    &CoreComponent,
    &[&RelocatedDylib],
) -> core::result::Result<(), Box<dyn Any + Send + Sync>>;

/// 在此之前检查是否需要relocate
pub(crate) fn relocate_impl<'iter, 'find, 'lib, F>(
    elf: ElfCommonPart,
    scope: Vec<&'iter RelocatedDylib>,
    pre_find: &'find F,
    deal_unknown: &mut DealUnknown,
    local_lazy_scope: Option<LazyScope<'lib>>,
) -> Result<Relocated<'lib>>
where
    F: Fn(&str) -> Option<*const ()>,
    'iter: 'lib,
    'find: 'lib,
{
    elf.relocate_relative()
        .relocate_dynrel(&scope, pre_find, deal_unknown)?
        .relocate_pltrel(local_lazy_scope, &scope, pre_find, deal_unknown)?
        .finish();
    Ok(Relocated {
        core: elf.into_core_component(),
        _marker: PhantomData,
    })
}

#[inline(always)]
fn write_val(base: usize, offset: usize, val: usize) {
    unsafe {
        let rel_addr = (base + offset) as *mut usize;
        rel_addr.write(val)
    };
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
        unsafe {
            core::mem::transmute::<usize, fn(&str) -> Option<*const ()>>(scope)(syminfo.name())
        }
        .or_else(|| dylib.lazy_scope.as_ref().unwrap()(syminfo.name()))
    }
    .expect("lazy bind fail") as usize;
    let ptr = (dylib.segments.base() + rela.r_offset()) as *mut usize;
    unsafe { ptr.write(symbol) };
    symbol
}

enum RelativeRel {
    Rel(&'static [ElfRelType]),
    Relr(&'static [ElfRelr]),
}

impl RelativeRel {
    #[inline]
    fn is_empty(&self) -> bool {
        match self {
            RelativeRel::Rel(rel) => rel.is_empty(),
            RelativeRel::Relr(relr) => relr.is_empty(),
        }
    }
}

pub(crate) struct ElfRelocation {
    // REL_RELATIVE
    relative: RelativeRel,
    // plt
    pltrel: &'static [ElfRelType],
    // others in dyn
    dynrel: &'static [ElfRelType],
}

fn find_symdef<'iter, 'temp>(
    core: &'temp CoreComponent,
    libs: &[&'iter RelocatedDylib],
    dynsym: &'temp ElfSymbol,
    syminfo: &SymbolInfo,
) -> Option<SymDef<'temp>>
where
    'iter: 'temp,
{
    if unlikely(dynsym.is_local()) {
        Some(SymDef {
            sym: Some(dynsym),
            base: core.base(),
        })
    } else {
        libs.iter()
            .find_map(|lib| {
                lib.symtab().lookup_filter(syminfo).map(|sym| {
                    #[cfg(feature = "log")]
                    log::trace!(
                        "binding file [{}] to [{}]: symbol [{}]",
                        core.name(),
                        lib.name(),
                        syminfo.name()
                    );
                    SymDef {
                        sym: Some(sym),
                        base: lib.base(),
                    }
                })
            })
            .or_else(|| {
                // 弱符号 + WEAK 用 0 填充rela offset
                if dynsym.is_weak() && dynsym.is_undef() {
                    assert!(dynsym.st_value() == 0);
                    Some(SymDef {
                        sym: None,
                        base: core.base(),
                    })
                } else if dynsym.st_value() != 0 {
                    Some(SymDef {
                        sym: Some(dynsym),
                        base: core.base(),
                    })
                } else {
                    None
                }
            })
    }
}

#[cold]
fn reloc_error(
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

impl ElfCommonPart {
    fn relocate_pltrel<F>(
        &self,
        local_lazy_scope: Option<LazyScope<'_>>,
        scope: &[&RelocatedDylib],
        pre_find: &F,
        deal_unknown: &mut DealUnknown,
    ) -> Result<&Self>
    where
        F: Fn(&str) -> Option<*const ()>,
    {
        let core = self.core_component_ref();
        let base = core.base();
        let reloc = self.relocation();
        let symbol = self.symtab().unwrap();
        if self.is_lazy() {
            // 开启lazy bind后会跳过plt相关的重定位
            for rel in reloc.pltrel {
                let r_type = rel.r_type() as u32;
                let r_addend = rel.r_addend(base);
                // S
                if likely(r_type == REL_JUMP_SLOT) {
                    let ptr = (base + rel.r_offset()) as *mut usize;
                    // 即使是延迟加载也需要进行简单重定位，好让plt代码能够正常工作
                    unsafe {
                        let origin_val = ptr.read();
                        let new_val = origin_val + base;
                        ptr.write(new_val);
                    }
                } else if unlikely(r_type == REL_IRELATIVE) {
                    let ifunc: fn() -> usize = unsafe { core::mem::transmute(base + r_addend) };
                    write_val(base, rel.r_offset(), ifunc());
                } else {
                    unreachable!()
                }
            }
            if !reloc.pltrel.is_empty() {
                prepare_lazy_bind(
                    self.got().unwrap().as_ptr(),
                    Arc::as_ptr(&core.inner) as usize,
                );
            }
            assert!(
                reloc.pltrel.is_empty()
                    || local_lazy_scope.is_some()
                    || GLOBAL_SCOPE.load(Ordering::Relaxed) != 0,
                "neither local lazy scope nor global scope is set"
            );
            if let Some(lazy_scope) = local_lazy_scope {
                core.set_lazy_scope(lazy_scope);
            }
        } else {
            for rel in reloc.pltrel {
                let r_type = rel.r_type() as u32;
                let r_sym = rel.r_symbol();
                let r_addend = rel.r_addend(base);
                // S
                // 对于.rela.plt来说通常只有这两种重定位类型
                if likely(r_type == REL_JUMP_SLOT) {
                    let (dynsym, syminfo) = symbol.symbol_idx(r_sym);
                    if let Some(symbol) = pre_find(syminfo.name()).or_else(|| {
                        find_symdef(core, scope, dynsym, &syminfo).map(|symdef| symdef.convert())
                    }) {
                        write_val(base, rel.r_offset(), symbol as usize);
                        continue;
                    }
                } else if unlikely(r_type == REL_IRELATIVE) {
                    let ifunc: fn() -> usize = unsafe { core::mem::transmute(base + r_addend) };
                    write_val(base, rel.r_offset(), ifunc());
                    continue;
                }
                deal_unknown(rel, core, scope)
                    .map_err(|err| reloc_error(r_type as _, r_sym, err, core))?;
            }
            if let Some(relro) = self.relro() {
                relro.relro()?;
            }
        }
        Ok(self)
    }

    fn relocate_relative(&self) -> &Self {
        let core = self.core_component_ref();
        let reloc = self.relocation();
        let base = core.base();
        match reloc.relative {
            RelativeRel::Rel(rel) => {
                assert!(rel.is_empty() || rel[0].r_type() == REL_RELATIVE as usize);
                rel.iter().for_each(|rel| {
                    // B + A
                    debug_assert!(rel.r_type() == REL_RELATIVE as usize);
                    let r_addend = rel.r_addend(base);
                    write_val(base, rel.r_offset(), base + r_addend);
                })
            }
            RelativeRel::Relr(relr) => {
                let mut reloc_addr: *mut usize = null_mut();
                relr.iter().for_each(|relr| {
                    let value = relr.value();
                    unsafe {
                        if (value & 1) == 0 {
                            reloc_addr = core.segments().get_mut_ptr(value);
                            reloc_addr.write(base + reloc_addr.read());
                            reloc_addr = reloc_addr.add(1);
                        } else {
                            let mut bitmap = value;
                            let mut idx = 0;
                            while bitmap != 0 {
                                bitmap >>= 1;
                                if (bitmap & 1) != 0 {
                                    let ptr = reloc_addr.add(idx);
                                    ptr.write(base + ptr.read());
                                }
                                idx += 1;
                            }
                            reloc_addr = reloc_addr.add(usize::BITS as usize - 1);
                        }
                    }
                });
            }
        }
        self
    }

    fn relocate_dynrel<F>(
        &self,
        scope: &[&RelocatedDylib],
        pre_find: &F,
        deal_unknown: &mut DealUnknown,
    ) -> Result<&Self>
    where
        F: Fn(&str) -> Option<*const ()>,
    {
        /*
            A Represents the addend used to compute the value of the relocatable field.
            B Represents the base address at which a shared object has been loaded into memory during execution.
            S Represents the value of the symbol whose index resides in the relocation entry.
        */

        let core = self.core_component_ref();
        let reloc = self.relocation();
        let symtab = self.symtab().unwrap();
        let base = core.base();
        for rel in reloc.dynrel {
            let r_type = rel.r_type() as _;
            let r_sym = rel.r_symbol();
            let r_addend = rel.r_addend(base);
            match r_type {
                // REL_GOT: S  REL_SYMBOLIC: S + A
                REL_GOT | REL_SYMBOLIC => {
                    let (dynsym, syminfo) = symtab.symbol_idx(r_sym);
                    if let Some(symbol) = pre_find(syminfo.name()).or_else(|| {
                        find_symdef(core, scope, dynsym, &syminfo).map(|symdef| symdef.convert())
                    }) {
                        write_val(base, rel.r_offset(), symbol as usize);
                        continue;
                    }
                }
                REL_DTPOFF => {
                    let (dynsym, syminfo) = symtab.symbol_idx(r_sym);
                    if let Some(symdef) = find_symdef(core, scope, dynsym, &syminfo) {
                        // offset in tls
                        let tls_val = (symdef.sym.unwrap().st_value() + r_addend)
                            .wrapping_sub(TLS_DTV_OFFSET);
                        write_val(base, rel.r_offset(), tls_val);
                        continue;
                    }
                }
                REL_COPY => {
                    let (dynsym, syminfo) = symtab.symbol_idx(r_sym);
                    if let Some(symbol) = find_symdef(core, scope, dynsym, &syminfo) {
                        let len = symbol.sym.unwrap().st_size();
                        let dest = core.segments().get_slice_mut::<u8>(rel.r_offset(), len);
                        let src = core
                            .segments()
                            .get_slice(symbol.sym.unwrap().st_value(), len);
                        dest.copy_from_slice(src);
                        continue;
                    }
                }
                _ => {}
            }
            if unlikely(r_type == REL_NONE) {
                continue;
            }
            deal_unknown(rel, core, scope)
                .map_err(|err| reloc_error(r_type as _, r_sym, err, core))?;
        }
        Ok(self)
    }
}

impl ElfRelocation {
    #[inline]
    pub(crate) fn new(
        pltrel: Option<&'static [ElfRelType]>,
        dynrel: Option<&'static [ElfRelType]>,
        relr: Option<&'static [ElfRelr]>,
        rela_count: Option<NonZeroUsize>,
    ) -> Self {
        if let Some(relr) = relr {
            Self {
                relative: RelativeRel::Relr(relr),
                pltrel: pltrel.unwrap_or(&[]),
                dynrel: dynrel.unwrap_or(&[]),
            }
        } else {
            // nrelative记录着REL_RELATIVE重定位类型的个数
            let nrelative = rela_count.map(|v| v.get()).unwrap_or(0);
            let old_dynrel = dynrel.unwrap_or(&[]);
            let relative = RelativeRel::Rel(&old_dynrel[..nrelative]);
            let temp_dynrel = &old_dynrel[nrelative..];
            let pltrel = pltrel.unwrap_or(&[]);
            let dynrel = if unsafe {
                core::ptr::eq(
                    old_dynrel.as_ptr().add(old_dynrel.len()),
                    pltrel.as_ptr().add(pltrel.len()),
                )
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
