//! Relocation of elf objects
use crate::{
    arch::*, relocate_error, symbol::SymbolInfo, CoreComponent, CoreComponentInner, ElfDylib,
    RelocatedDylib, Result,
};
use alloc::{boxed::Box, format, sync::Arc};
use core::{marker::PhantomData, num::NonZeroUsize, sync::atomic::AtomicUsize};
use elf::abi::*;

pub(crate) static GLOBAL_SCOPE: AtomicUsize = AtomicUsize::new(0);

#[allow(unused)]
struct SymDef<'temp> {
    sym: &'temp ElfSymbol,
    base: usize,
}

impl<'temp> SymDef<'temp> {
    #[inline(always)]
    fn convert(self) -> *const () {
        if likely(self.sym.st_info & 0xf != STT_GNU_IFUNC) {
            (self.base + self.sym.st_value as usize) as _
        } else {
            let ifunc: fn() -> usize =
                unsafe { core::mem::transmute(self.base + self.sym.st_value as usize) };
            ifunc() as _
        }
    }
}

impl ElfDylib {
    /// Relocate the dynamic library with the given dynamic libraries and function closure.
    /// # Note
    /// * During relocation, it is preferred to look for symbols in function closures `find`.
    /// * The `deal_unknown` function is used to handle relocation types not implemented by efl_loader or failed relocations
    /// * Typically, the `scope` should also contain the current dynamic library itself,
    /// relocation will be done in the exact order in which the dynamic libraries appear in `scope`.
    /// * When lazy binding, the symbol is first looked for in the global scope and then in the lazy scope
    pub fn relocate<'scope, S, F, D>(
        self,
        scope: S,
        find: &'scope F,
        deal_unknown: D,
        lazy_scope: Option<Box<dyn for<'a> Fn(&'a str) -> Option<*const ()> + 'static>>,
    ) -> Result<RelocatedDylib<'scope>>
    where
        S: Iterator<Item = &'scope CoreComponent> + Clone,
        F: Fn(&str) -> Option<*const ()>,
        D: Fn(&ElfRela, &ElfDylib, S) -> bool,
    {
        fn find_symdef<'a, 'scope: 'a, I>(
            elf_lib: &'a CoreComponentInner,
            mut libs: I,
            dynsym: &'a ElfSymbol,
            syminfo: &SymbolInfo,
        ) -> Option<SymDef<'a>>
        where
            I: Iterator<Item = &'scope CoreComponent>,
        {
            if unlikely(dynsym.st_info >> 4 == STB_LOCAL) {
                Some(SymDef {
                    sym: dynsym,
                    base: elf_lib.segments.base(),
                })
            } else {
                libs.find_map(|lib| {
                    lib.inner.symbols.lookup_filter(&syminfo).map(|sym| SymDef {
                        sym,
                        base: lib.base(),
                    })
                })
            }
        }

        /*
            A Represents the addend used to compute the value of the relocatable field.
            B Represents the base address at which a shared object has been loaded into memory during execution.
            S Represents the value of the symbol whose index resides in the relocation entry.
        */

        if let Some(rela_array) = self.relocation.relative {
            assert!(rela_array[0].r_type() == REL_RELATIVE as usize);
            rela_array.iter().for_each(|rela| {
                // B + A
                debug_assert!(rela.r_type() == REL_RELATIVE as usize);
                self.write_val(rela.r_offset(), self.core.base() + rela.r_addend());
            });
        }

        if let Some(rela_array) = self.relocation.dynrel {
            for rela in rela_array {
                let r_type = rela.r_type() as _;
                let r_sym = rela.r_symbol();

                if unlikely(r_type == REL_RELATIVE) {
                    self.write_val(rela.r_offset(), self.core.base() + rela.r_addend());
                    continue;
                } else if unlikely(r_type == REL_NONE) {
                    continue;
                }

                match r_type {
                    // REL_GOT: S  REL_SYMBOLIC: S + A
                    REL_GOT | REL_SYMBOLIC => {
                        let (dynsym, syminfo) = self.core.symtab().symbol_idx(r_sym);
                        if let Some(symbol) = find(syminfo.name).or(find_symdef(
                            &self.core.inner,
                            scope.clone(),
                            dynsym,
                            &syminfo,
                        )
                        .map(|symdef| symdef.convert()))
                        {
                            self.write_val(rela.r_offset(), symbol as usize);
                            continue;
                        }
                    }
                    REL_DTPOFF => {
                        let (dynsym, syminfo) = self.core.symtab().symbol_idx(r_sym);
                        if let Some(symdef) =
                            find_symdef(&self.core.inner, scope.clone(), dynsym, &syminfo)
                        {
                            // offset in tls
                            let tls_val = (symdef.sym.st_value as usize + rela.r_addend())
                                .wrapping_sub(TLS_DTV_OFFSET);
                            self.write_val(rela.r_offset(), tls_val);
                            continue;
                        }
                    }
                    _ => {}
                }
                if unlikely(!deal_unknown(&rela, &self, scope.clone())) {
                    return Err(relocate_error(format!(
                        "unsupported relocation type: {}, symbol idx:{}",
                        r_type, r_sym,
                    )));
                }
            }
        }

        if let Some(rela_array) = self.relocation.pltrel {
            // 开启lazy bind后会跳过plt相关的重定位
            if self.lazy {
                for rela in rela_array {
                    let r_type = rela.r_type() as u32;
                    // S
                    if likely(r_type == REL_JUMP_SLOT) {
                        let ptr = (self.base() + rela.r_offset()) as *mut usize;
                        // 即使是延迟加载也需要进行简单重定位，好让plt代码能够正常工作
                        unsafe {
                            let origin_val = ptr.read();
                            let new_val = origin_val + self.base();
                            ptr.write(new_val);
                        }
                    } else if unlikely(r_type == REL_IRELATIVE) {
                        let ifunc: fn() -> usize =
                            unsafe { core::mem::transmute(self.base() + rela.r_addend()) };
                        self.write_val(rela.r_offset(), ifunc());
                    } else {
                        unreachable!()
                    }
                }
                if let Some(got) = self.got {
                    prepare_lazy_bind(got, Arc::as_ptr(&self.core.inner) as usize);
                }
                // 因为在完成重定位前，只有unsafe的方法可以拿到CoreComponent的引用，所以这里认为是安全的
                let ptr =
                    unsafe { &mut *(Arc::as_ptr(&self.core.inner) as *mut CoreComponentInner) };
                ptr.lazy_scope = lazy_scope;
            } else {
                for rela in rela_array {
                    let r_type = rela.r_type() as u32;
                    let r_sym = rela.r_symbol();
                    // S
                    // 对于.rela.plt来说通常只有这两种重定位类型
                    if likely(r_type == REL_JUMP_SLOT) {
                        let (dynsym, syminfo) = self.core.symtab().symbol_idx(r_sym);
                        if let Some(symbol) = find(syminfo.name).or(find_symdef(
                            &self.core.inner,
                            scope.clone(),
                            dynsym,
                            &syminfo,
                        )
                        .map(|symdef| symdef.convert()))
                        {
                            self.write_val(rela.r_offset(), symbol as usize);
                            continue;
                        }
                    } else if unlikely(r_type == REL_IRELATIVE) {
                        let ifunc: fn() -> usize =
                            unsafe { core::mem::transmute(self.base() + rela.r_addend()) };
                        self.write_val(rela.r_offset(), ifunc());
                        continue;
                    }
                    if unlikely(!deal_unknown(&rela, &self, scope.clone())) {
                        return Err(relocate_error(format!(
                            "unsupported relocation type: {}, symbol idx:{}",
                            r_type, r_sym,
                        )));
                    }
                }
                if let Some(relro) = self.relro {
                    relro.relro()?;
                }
            }
        }

        if let Some(init) = self.init_fn {
            init();
        }
        if let Some(init_array) = self.init_array_fn {
            for init in init_array {
                init();
            }
        }

        Ok(RelocatedDylib {
            core: self.core,
            _marker: PhantomData,
        })
    }

    #[inline(always)]
    fn write_val(&self, offset: usize, val: usize) {
        unsafe {
            let rel_addr = (self.core.base() + offset) as *mut usize;
            rel_addr.write(val)
        };
    }
}

#[no_mangle]
unsafe extern "C" fn dl_fixup(dylib: &CoreComponentInner, rela_idx: usize) -> usize {
    let rela = &*dylib.pltrel.add(rela_idx);
    let r_type = rela.r_type();
    let r_sym = rela.r_symbol();
    assert!(r_type == REL_JUMP_SLOT as usize && r_sym != 0);
    let (_, syminfo) = dylib.symbols.symbol_idx(r_sym);
    let scope = GLOBAL_SCOPE.load(core::sync::atomic::Ordering::Acquire);
    let symbol = if scope == 0 {
        dylib.lazy_scope.as_ref().unwrap()(syminfo.name)
    } else {
        core::mem::transmute::<_, fn(&str) -> Option<*const ()>>(scope)(syminfo.name).or(dylib
            .lazy_scope
            .as_ref()
            .unwrap()(
            syminfo.name,
        ))
    }
    .expect("lazy bind fail") as usize;
    let ptr = (dylib.segments.base() + rela.r_offset()) as *mut usize;
    ptr.write(symbol);
    symbol
}

#[derive(Default)]
pub(crate) struct ElfRelocation {
    relative: Option<&'static [ElfRela]>,
    pltrel: Option<&'static [ElfRela]>,
    dynrel: Option<&'static [ElfRela]>,
}

impl ElfRelocation {
    #[inline]
    pub(crate) fn new(
        pltrel: Option<&'static [ElfRela]>,
        dynrel: Option<&'static [ElfRela]>,
        rela_count: Option<NonZeroUsize>,
    ) -> Self {
        let (relative, dynrel) = if let Some(nrelative) = rela_count {
            unsafe {
                (
                    Some(&dynrel.unwrap_unchecked()[..nrelative.get()]),
                    Some(&dynrel.unwrap_unchecked()[nrelative.get()..]),
                )
            }
        } else {
            (None, dynrel)
        };
        let dynrel = if let (Some(dynrel), Some(pltrel)) = (dynrel, pltrel) {
            if unsafe { dynrel.as_ptr().add(dynrel.len()) == pltrel.as_ptr().add(pltrel.len()) } {
                Some(&dynrel[..dynrel.len() - pltrel.len()])
            } else {
                Some(dynrel)
            }
        } else {
            None
        };
        Self {
            relative,
            pltrel,
            dynrel,
        }
    }

    #[inline]
    pub(crate) fn pltrel(&self) -> Option<&[ElfRela]> {
        self.pltrel
    }

    #[inline]
    pub(crate) fn dynrel(&self) -> Option<&[ElfRela]> {
        self.dynrel
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
