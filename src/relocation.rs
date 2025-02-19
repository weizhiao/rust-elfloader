//! Relocation of elf objects
use crate::{
    CoreComponentInner, ElfDylib, Error, RelocatedDylib, Result, arch::*, relocate_error,
    symbol::SymbolInfo,
};
use alloc::{boxed::Box, format, sync::Arc};
use core::{
    any::Any,
    ffi::c_int,
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

impl ElfDylib {
    /// Relocate the dynamic library with the given dynamic libraries and function closure.
    /// # Note
    /// During relocation, the symbol is first searched in the function closure `pre_find`.
    pub fn easy_relocate<'iter, 'scope, 'find, 'lib, S, F>(
        self,
        scope: S,
        pre_find: &'find F,
    ) -> Result<RelocatedDylib<'lib>>
    where
        S: Iterator<Item = &'iter RelocatedDylib<'scope>> + Clone,
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        self.relocate(scope, pre_find, |_, _, _| Err(Box::new(())), None)
    }

    /// Relocate the dynamic library with the given dynamic libraries and function closure.
    /// # Note
    /// * During relocation, the symbol is first searched in the function closure `pre_find`.
    /// * The `deal_unknown` function is used to handle relocation types not implemented by efl_loader or failed relocations
    /// * Typically, the `scope` should also contain the current dynamic library itself,
    /// relocation will be done in the exact order in which the dynamic libraries appear in `scope`.
    /// * When lazy binding, the symbol is first looked for in the global scope and then in the local lazy scope
    pub fn relocate<'iter, 'scope, 'find, 'lib, S, F, D>(
        self,
        scope: S,
        pre_find: &'find F,
        deal_unknown: D,
        local_lazy_scope: Option<Box<dyn for<'a> Fn(&'a str) -> Option<*const ()> + 'static>>,
    ) -> Result<RelocatedDylib<'lib>>
    where
        S: Iterator<Item = &'iter RelocatedDylib<'scope>> + Clone,
        F: Fn(&str) -> Option<*const ()>,
        D: Fn(&ElfRela, &ElfDylib, S) -> core::result::Result<(), Box<dyn Any>>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        fn find_symdef<'iter, 'temp, 'scope, I>(
            elf_lib: &'temp ElfDylib,
            mut libs: I,
            dynsym: &'temp ElfSymbol,
            syminfo: &SymbolInfo,
        ) -> Option<SymDef<'temp>>
        where
            'scope: 'temp,
            'scope: 'iter,
            'iter: 'temp,
            I: Iterator<Item = &'iter RelocatedDylib<'scope>>,
        {
            if unlikely(dynsym.is_local()) {
                Some(SymDef {
                    sym: dynsym,
                    base: elf_lib.base(),
                })
            } else {
                libs.find_map(|lib| {
                    lib.symtab().lookup_filter(&syminfo).map(|sym| SymDef {
                        sym,
                        base: lib.base(),
                    })
                })
            }
        }

        #[cold]
        fn reloc_error(
            r_type: usize,
            r_sym: usize,
            custom_err: Box<dyn Any>,
            lib: &ElfDylib,
        ) -> Error {
            if r_sym == 0 {
                relocate_error(
                    format!(
                        "dylib: {}, relocation type: {}, no symbol",
                        lib.shortname(),
                        r_type,
                    ),
                    custom_err,
                )
            } else {
                relocate_error(
                    format!(
                        "dylib: {}, relocation type: {}, symbol name: {}",
                        lib.shortname(),
                        r_type,
                        lib.symtab().symbol_idx(r_sym).1.name(),
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

        assert!(
            !(self.relocation.relative.len() > 0
                && self.relocation.relative[0].r_type() != REL_RELATIVE as usize)
        );
        self.relocation.relative.into_iter().for_each(|rela| {
            // B + A
            debug_assert!(rela.r_type() == REL_RELATIVE as usize);
            self.write_val(rela.r_offset(), self.core.base() + rela.r_addend());
        });

        for rela in self.relocation.dynrel {
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
                    let (dynsym, syminfo) = self.symtab().symbol_idx(r_sym);
                    if let Some(symbol) = pre_find(syminfo.name()).or(find_symdef(
                        &self,
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
                    let (dynsym, syminfo) = self.symtab().symbol_idx(r_sym);
                    if let Some(symdef) = find_symdef(&self, scope.clone(), dynsym, &syminfo) {
                        // offset in tls
                        let tls_val =
                            (symdef.sym.st_value() + rela.r_addend()).wrapping_sub(TLS_DTV_OFFSET);
                        self.write_val(rela.r_offset(), tls_val);
                        continue;
                    }
                }
                _ => {}
            }
            deal_unknown(&rela, &self, scope.clone())
                .map_err(|err| reloc_error(r_type as _, r_sym, err, &self))?;
        }

        // 开启lazy bind后会跳过plt相关的重定位
        if self.lazy {
            for rela in self.relocation.pltrel {
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
            assert!(
                self.relocation.pltrel.is_empty()
                    || local_lazy_scope.is_some()
                    || GLOBAL_SCOPE.load(Ordering::Relaxed) != 0,
                "neither local lazy scope nor global scope is set"
            );
            self.core.set_lazy_scope(local_lazy_scope);
        } else {
            for rela in self.relocation.pltrel {
                let r_type = rela.r_type() as u32;
                let r_sym = rela.r_symbol();
                // S
                // 对于.rela.plt来说通常只有这两种重定位类型
                if likely(r_type == REL_JUMP_SLOT) {
                    let (dynsym, syminfo) = self.symtab().symbol_idx(r_sym);
                    if let Some(symbol) = pre_find(syminfo.name()).or(find_symdef(
                        &self,
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
                deal_unknown(&rela, &self, scope.clone())
                    .map_err(|err| reloc_error(r_type as _, r_sym, err, &self))?;
            }
            if let Some(relro) = self.relro {
                relro.relro()?;
            }
        }

        if let Some(init_params) = self.init_params {
            self.init_fn
                .iter()
                .chain(self.init_array_fn.unwrap_or(&[]).iter())
                .for_each(|init| unsafe {
                    core::mem::transmute::<_, extern "C" fn(c_int, usize, usize)>(*init)(
                        init_params.argc as _,
                        init_params.argv,
                        init_params.envp,
                    );
                });
        } else {
            self.init_fn
                .iter()
                .chain(self.init_array_fn.unwrap_or(&[]).iter())
                .for_each(|init| init());
        }

        self.core.set_init();
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

#[unsafe(no_mangle)]
unsafe extern "C" fn dl_fixup(dylib: &CoreComponentInner, rela_idx: usize) -> usize {
    let rela = unsafe { &*dylib.pltrel.add(rela_idx) };
    let r_type = rela.r_type();
    let r_sym = rela.r_symbol();
    assert!(r_type == REL_JUMP_SLOT as usize && r_sym != 0);
    let (_, syminfo) = dylib.symbols.symbol_idx(r_sym);
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
