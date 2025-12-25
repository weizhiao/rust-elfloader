//! Relocation of elf objects
use crate::{
    ElfModuleRef, LoadedDylib, Result,
    arch::*,
    format::{DynamicComponent, LoadedModule, ModuleInner},
    relocation::{
        RelocContext, RelocValue, RelocationContext, RelocationHandler, SymbolLookup,
        find_symbol_addr, likely, reloc_error, unlikely,
    },
};
use alloc::vec::Vec;
use core::{num::NonZeroUsize, ptr::null_mut};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

/// LazyScope holds both the local scope lookup and an optional parent scope
/// This avoids requiring D to be 'static by storing weak references to libraries
struct LazyScope<D = (), S: SymbolLookup = ()>
where
    S: SymbolLookup,
{
    /// Weak references to the local libraries for symbol lookup
    libs: Vec<ElfModuleRef<D>>,
    custom_scope: Option<S>,
}

impl<D, S: SymbolLookup> SymbolLookup for LazyScope<D, S> {
    fn lookup(&self, name: &str) -> Option<*const ()> {
        // First try the parent scope if available
        if let Some(parent) = &self.custom_scope {
            if let Some(sym) = parent.lookup(name) {
                return Some(sym);
            }
        }
        // Then try the local libraries
        self.libs.iter().find_map(|lib| unsafe {
            let core = lib.upgrade()?;
            LoadedDylib::from_core(core)
                .get::<()>(name)
                .map(|sym| sym.into_raw())
        })
    }
}

/// Resolve indirect function address
///
/// # Safety
/// The address must point to a valid IFUNC function.
#[inline(always)]
unsafe fn resolve_ifunc(addr: RelocValue<usize>) -> RelocValue<usize> {
    let ifunc: fn() -> usize = unsafe { core::mem::transmute(addr.0) };
    RelocValue::new(ifunc())
}

impl<D> DynamicComponent<D> {
    pub(crate) fn relocate_impl<PreS, PostS, LazyS, PreH, PostH>(
        self,
        scope: &[LoadedModule<D>],
        pre_find: &PreS,
        post_find: &PostS,
        mut pre_handler: PreH,
        mut post_handler: PostH,
        lazy: Option<bool>,
        lazy_scope: Option<LazyS>,
        use_scope_as_lazy: bool,
    ) -> Result<(LoadedModule<D>, usize)>
    where
        D: 'static,
        PreS: SymbolLookup + ?Sized,
        PostS: SymbolLookup + ?Sized,
        LazyS: SymbolLookup + Send + Sync + 'static,
        PreH: RelocationHandler,
        PostH: RelocationHandler,
    {
        // Optimization: check if relocation is empty
        if self.relocation().is_empty() {
            let entry = self.entry();
            let core = self.into_core();
            let relocated = unsafe { LoadedModule::from_core(core) };
            return Ok((relocated, entry));
        }

        let is_lazy = lazy.unwrap_or(self.is_lazy());
        let entry = self.entry();

        let lazy_scope = if is_lazy {
            if use_scope_as_lazy {
                let libs = scope.iter().map(|lib| lib.downgrade()).collect();
                Some(LazyScope {
                    libs,
                    custom_scope: lazy_scope,
                })
            } else {
                Some(LazyScope {
                    libs: Vec::new(),
                    custom_scope: lazy_scope,
                })
            }
        } else {
            None
        };

        let relocated = relocate_impl(
            self,
            scope,
            pre_find,
            post_find,
            &mut pre_handler,
            &mut post_handler,
            is_lazy,
            lazy_scope,
        )?;

        Ok((relocated, entry))
    }
}

/// Perform relocations on an ELF object
fn relocate_impl<D, PreS, PostS, LazyS, PreH, PostH>(
    elf: DynamicComponent<D>,
    scope: &[LoadedModule<D>],
    pre_find: &PreS,
    post_find: &PostS,
    pre_handler: &mut PreH,
    post_handler: &mut PostH,
    is_lazy: bool,
    lazy_scope: Option<LazyS>,
) -> Result<LoadedModule<D>>
where
    PreS: SymbolLookup + ?Sized,
    PostS: SymbolLookup + ?Sized,
    LazyS: SymbolLookup + Send + Sync + 'static,
    PreH: RelocationHandler + ?Sized,
    PostH: RelocationHandler + ?Sized,
{
    let mut ctx = RelocContext {
        scope,
        pre_find,
        post_find,
        pre_handler,
        post_handler,
        dependency_flags: alloc::vec![false; scope.len()],
    };
    elf.relocate_relative()
        .relocate_dynrel(&mut ctx)?
        .relocate_pltrel(is_lazy, lazy_scope, &mut ctx)?
        .finish();
    let deps: Vec<LoadedModule<D>> = ctx
        .dependency_flags
        .iter()
        .enumerate()
        .filter_map(|(i, &flag)| if flag { Some(scope[i].clone()) } else { None })
        .collect();
    Ok(unsafe { LoadedModule::from_core_deps(elf.into_core(), deps) })
}

/// Lazy binding fixup function called by PLT (Procedure Linkage Table)
pub(crate) unsafe extern "C" fn dl_fixup(dylib: &ModuleInner, rela_idx: usize) -> usize {
    // Get the relocation entry for this function call
    let rela = unsafe {
        &*dylib
            .dynamic_info
            .as_ref()
            .unwrap()
            .pltrel
            .unwrap()
            .add(rela_idx)
            .as_ptr()
    };
    let r_type = rela.r_type();
    let r_sym = rela.r_symbol();
    let segments = &dylib.segments;

    // Ensure this is a jump slot relocation for a valid symbol
    assert!(r_type == REL_JUMP_SLOT as usize && r_sym != 0);

    // Get symbol information
    let (_, syminfo) = dylib.symbols.as_ref().unwrap().symbol_idx(r_sym);

    // Look up symbol in local scope
    let symbol = dylib
        .dynamic_info
        .as_ref()
        .unwrap()
        .lazy_scope
        .as_ref()
        .unwrap()
        .lookup(syminfo.name())
        .expect("lazy bind fail") as usize;

    // Write the resolved symbol address to the GOT entry
    segments.write(rela.r_offset(), RelocValue::new(symbol));
    symbol
}

/// Types of relative relocations
enum RelativeRel {
    /// Standard REL/RELA relocations
    Rel(&'static [ElfRelType]),
    /// Compact RELR relocations
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

/// Holds parsed relocation information
pub(crate) struct DynamicRelocation {
    /// Relative relocations (REL_RELATIVE)
    relative: RelativeRel,
    /// PLT relocations
    pltrel: &'static [ElfRelType],
    /// Other dynamic relocations
    dynrel: &'static [ElfRelType],
}

impl<D> DynamicComponent<D> {
    /// Relocate PLT (Procedure Linkage Table) entries
    fn relocate_pltrel<PreS, PostS, LazyS, PreH, PostH>(
        &self,
        is_lazy: bool,
        lazy_scope: Option<LazyS>,
        ctx: &mut RelocContext<'_, '_, D, PreS, PostS, PreH, PostH>,
    ) -> Result<&Self>
    where
        PreS: SymbolLookup + ?Sized,
        PostS: SymbolLookup + ?Sized,
        LazyS: SymbolLookup + Send + Sync + 'static,
        PreH: RelocationHandler + ?Sized,
        PostH: RelocationHandler + ?Sized,
    {
        let scope = ctx.scope;
        let pre_find = ctx.pre_find;
        let post_find = ctx.post_find;
        let core = self.core_ref();
        let base = core.base();
        let segments = core.segments();
        let reloc = self.relocation();
        let symtab = self.symtab().unwrap();

        // Process PLT relocations
        for rel in reloc.pltrel {
            let hctx = RelocationContext::new(rel, core, scope);
            if !ctx.handle_pre(&hctx)? {
                continue;
            }
            let r_type = rel.r_type() as u32;
            let r_sym = rel.r_symbol();
            let r_addend = rel.r_addend(base);

            // Handle jump slot relocations
            if likely(r_type == REL_JUMP_SLOT) {
                if is_lazy {
                    let addr = RelocValue::new(base) + rel.r_offset();
                    let ptr = addr.as_mut_ptr::<usize>();
                    // Even with lazy binding, basic relocation is needed for PLT to work
                    unsafe {
                        let origin_val = ptr.read();
                        let new_val = origin_val + base;
                        ptr.write(new_val);
                    }
                } else {
                    if let Some((symbol, idx)) =
                        find_symbol_addr(pre_find, post_find, core, symtab, scope, r_sym)
                    {
                        if let Some(idx) = idx {
                            ctx.dependency_flags[idx] = true;
                        }
                        segments.write(rel.r_offset(), symbol);
                    }
                }
                continue;
            } else if unlikely(r_type == REL_IRELATIVE) {
                // Handle indirect function relocations
                let addr = RelocValue::new(base) + r_addend;
                segments.write(rel.r_offset(), unsafe { resolve_ifunc(addr) });
                continue;
            }
            // Handle unknown relocations with the provided handler
            if ctx.handle_post(&hctx)? {
                return Err(reloc_error(rel, "Unhandled relocation", core));
            }
        }

        if is_lazy {
            // Prepare for lazy binding if we have PLT relocations
            if !reloc.pltrel.is_empty() {
                prepare_lazy_bind(
                    self.got().unwrap().as_ptr(),
                    Arc::as_ptr(&core.inner) as usize,
                );
            }

            // Ensure lazy scope is available
            assert!(
                reloc.pltrel.is_empty() || lazy_scope.is_some(),
                "{}: lazy scope is not set, please call `use_scope_as_lazy()` or provide a lazy scope",
                core.name()
            );

            // Set the lazy scope if provided
            if let Some(lazy_scope) = lazy_scope {
                core.set_lazy_scope(lazy_scope);
            }
        } else {
            // Apply RELRO (RELocation Read-Only) protection if available
            if let Some(relro) = self.relro() {
                relro.relro()?;
            }
        }
        Ok(self)
    }

    /// Perform relative relocations (REL_RELATIVE)
    fn relocate_relative(&self) -> &Self {
        let core = self.core_ref();
        let reloc = self.relocation();
        let segments = core.segments();
        let base = core.base();

        match reloc.relative {
            RelativeRel::Rel(rel) => {
                assert!(rel.is_empty() || rel[0].r_type() == REL_RELATIVE as usize);
                // Apply all relative relocations: new_value = base_address + addend
                rel.iter().for_each(|rel| {
                    debug_assert!(rel.r_type() == REL_RELATIVE as usize);
                    let r_addend = rel.r_addend(base);
                    let val = RelocValue::new(base) + r_addend;
                    segments.write(rel.r_offset(), val);
                })
            }
            RelativeRel::Relr(relr) => {
                // Apply compact relative relocations (RELR format)
                let mut reloc_addr: *mut usize = null_mut();
                relr.iter().for_each(|relr| {
                    let value = relr.value();
                    unsafe {
                        if (value & 1) == 0 {
                            // Single relocation entry
                            reloc_addr = core.segments().get_mut_ptr(value);
                            reloc_addr.write(base + reloc_addr.read());
                            reloc_addr = reloc_addr.add(1);
                        } else {
                            // Bitmap of relocations
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

    /// Perform dynamic relocations (non-PLT, non-relative)
    fn relocate_dynrel<PreS, PostS, PreH, PostH>(
        &self,
        ctx: &mut RelocContext<'_, '_, D, PreS, PostS, PreH, PostH>,
    ) -> Result<&Self>
    where
        PreS: SymbolLookup + ?Sized,
        PostS: SymbolLookup + ?Sized,
        PreH: RelocationHandler + ?Sized,
        PostH: RelocationHandler + ?Sized,
    {
        let scope = ctx.scope;
        let pre_find = ctx.pre_find;
        let post_find = ctx.post_find;
        /*
            Relocation formula components:
            A = Addend used to compute the value of the relocatable field
            B = Base address at which a shared object is loaded
            S = Value of the symbol whose index resides in the relocation entry
        */

        let core = self.core_ref();
        let reloc = self.relocation();
        let symtab = self.symtab().unwrap();
        let segments = core.segments();
        let base = core.base();

        // Process each dynamic relocation entry
        for rel in reloc.dynrel {
            let hctx = RelocationContext::new(rel, core, scope);
            if !ctx.handle_pre(&hctx)? {
                continue;
            }
            let r_type = rel.r_type() as _;
            let r_sym = rel.r_symbol();
            let r_addend = rel.r_addend(base);

            match r_type {
                // Handle GOT and symbolic relocations
                REL_GOT | REL_SYMBOLIC => {
                    if let Some((symbol, idx)) =
                        find_symbol_addr(pre_find, post_find, core, symtab, scope, r_sym)
                    {
                        if let Some(idx) = idx {
                            ctx.dependency_flags[idx] = true;
                        }
                        segments.write(rel.r_offset(), symbol + r_addend);
                        continue;
                    }
                }
                // Handle TLS (Thread Local Storage) offset relocations
                REL_DTPOFF => {
                    if let Some((symdef, idx)) = hctx.find_symdef(r_sym) {
                        if let Some(idx) = idx {
                            ctx.dependency_flags[idx] = true;
                        }
                        // Calculate offset within TLS block
                        let tls_val = RelocValue::new(symdef.sym.unwrap().st_value()) + r_addend
                            - TLS_DTV_OFFSET;
                        segments.write(rel.r_offset(), tls_val);
                        continue;
                    }
                }
                // Handle copy relocations (typically for global data)
                REL_COPY => {
                    if let Some((symdef, idx)) = hctx.find_symdef(r_sym) {
                        let len = hctx.lib.symtab().unwrap().symbol_idx(r_sym).0.st_size();
                        if let Some(idx) = idx {
                            ctx.dependency_flags[idx] = true;
                        }
                        let dest = core.segments().get_slice_mut::<u8>(rel.r_offset(), len);
                        let src = symdef
                            .lib
                            .segments()
                            .get_slice(symdef.sym.unwrap().st_value(), len);
                        dest.copy_from_slice(src);
                        continue;
                    }
                }
                REL_IRELATIVE => {
                    // Handle indirect function relocations
                    let addr = RelocValue::new(base) + r_addend;
                    segments.write(rel.r_offset(), unsafe { resolve_ifunc(addr) });
                    continue;
                }
                // No relocation needed
                REL_NONE => continue,
                // Unknown relocation type
                _ => {}
            }

            // Handle unknown relocations with the provided handler
            if ctx.handle_post(&hctx)? {
                return Err(reloc_error(rel, "Unhandled relocation", core));
            }
        }
        Ok(self)
    }
}

impl DynamicRelocation {
    /// Create a new DynamicRelocation instance from parsed relocation data
    #[inline]
    pub(crate) fn new(
        pltrel: Option<&'static [ElfRelType]>,
        dynrel: Option<&'static [ElfRelType]>,
        relr: Option<&'static [ElfRelr]>,
        rela_count: Option<NonZeroUsize>,
    ) -> Self {
        if let Some(relr) = relr {
            // Use RELR relocations if available (more compact format)
            Self {
                relative: RelativeRel::Relr(relr),
                pltrel: pltrel.unwrap_or(&[]),
                dynrel: dynrel.unwrap_or(&[]),
            }
        } else {
            // Use traditional REL/RELA relocations
            // nrelative indicates the count of REL_RELATIVE relocation types
            let nrelative = rela_count.map(|v| v.get()).unwrap_or(0);
            let old_dynrel = dynrel.unwrap_or(&[]);

            // Split relocations into relative and non-relative parts
            let relative = RelativeRel::Rel(&old_dynrel[..nrelative]);
            let temp_dynrel = &old_dynrel[nrelative..];

            let pltrel = pltrel.unwrap_or(&[]);
            let dynrel = if unsafe {
                // Check if dynrel and pltrel are contiguous in memory
                core::ptr::eq(
                    old_dynrel.as_ptr().add(old_dynrel.len()),
                    pltrel.as_ptr().add(pltrel.len()),
                )
            } {
                // If contiguous, exclude pltrel entries from dynrel
                &temp_dynrel[..temp_dynrel.len() - pltrel.len()]
            } else {
                // Otherwise, use all remaining entries
                temp_dynrel
            };

            Self {
                relative,
                pltrel,
                dynrel,
            }
        }
    }

    /// Check if there are no relocations to process
    #[inline]
    fn is_empty(&self) -> bool {
        self.relative.is_empty() && self.dynrel.is_empty() && self.pltrel.is_empty()
    }
}
