//! Relocation of elf objects
use crate::{
    CoreComponent, Result,
    arch::*,
    format::{Relocated, relocated::RelocatedCommonPart},
    relocation::{find_symbol_addr, find_symdef, likely, reloc_error, unlikely, write_val},
};
use alloc::boxed::Box;
use core::{
    any::Any,
    marker::PhantomData,
    num::NonZeroUsize,
    ptr::null_mut,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::Arc;

/// Global scope for lazy binding - symbols will be looked up here first
pub(crate) static GLOBAL_SCOPE: AtomicUsize = AtomicUsize::new(0);

pub(crate) type LazyScope<'lib> = Arc<dyn for<'a> Fn(&'a str) -> Option<*const ()> + 'lib>;

pub(crate) type UnknownHandler = dyn FnMut(
    &ElfRelType,
    &CoreComponent,
    &[&Relocated],
) -> core::result::Result<(), Box<dyn Any + Send + Sync>>;

/// Perform relocations on an ELF object
pub(crate) fn relocate_impl<'iter, 'find, 'lib, F>(
    elf: RelocatedCommonPart,
    scope: &[&'iter Relocated],
    pre_find: &'find F,
    deal_unknown: &mut UnknownHandler,
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

/// Lazy binding fixup function called by PLT (Procedure Linkage Table)
#[unsafe(no_mangle)]
unsafe extern "C" fn dl_fixup(dylib: &crate::format::CoreComponentInner, rela_idx: usize) -> usize {
    // Get the relocation entry for this function call
    let rela = unsafe { &*dylib.pltrel.unwrap().add(rela_idx).as_ptr() };
    let r_type = rela.r_type();
    let r_sym = rela.r_symbol();

    // Ensure this is a jump slot relocation for a valid symbol
    assert!(r_type == REL_JUMP_SLOT as usize && r_sym != 0);

    // Get symbol information
    let (_, syminfo) = dylib.symbols.as_ref().unwrap().symbol_idx(r_sym);

    // Look up symbol in global scope first, then local scope
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

    // Write the resolved symbol address to the GOT entry
    let ptr = (dylib.segments.base() + rela.r_offset()) as *mut usize;
    unsafe { ptr.write(symbol) };
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

impl RelocatedCommonPart {
    /// Relocate PLT (Procedure Linkage Table) entries
    fn relocate_pltrel<F>(
        &self,
        local_lazy_scope: Option<LazyScope<'_>>,
        scope: &[&Relocated],
        pre_find: &F,
        deal_unknown: &mut UnknownHandler,
    ) -> Result<&Self>
    where
        F: Fn(&str) -> Option<*const ()>,
    {
        let core = self.core_component_ref();
        let base = core.base();
        let reloc = self.relocation();
        let symtab = self.symtab().unwrap();
        let is_lazy = self.is_lazy();

        if is_lazy {
            // With lazy binding, skip full PLT relocations but do minimal setup
            for rel in reloc.pltrel {
                let r_type = rel.r_type() as u32;
                let r_addend = rel.r_addend(base);

                // Handle jump slot relocations
                if likely(r_type == REL_JUMP_SLOT) {
                    let ptr = (base + rel.r_offset()) as *mut usize;
                    // Even with lazy binding, basic relocation is needed for PLT to work
                    unsafe {
                        let origin_val = ptr.read();
                        let new_val = origin_val + base;
                        ptr.write(new_val);
                    }
                } else if unlikely(r_type == REL_IRELATIVE) {
                    // Handle indirect function relocations
                    let ifunc: fn() -> usize =
                        unsafe { core::mem::transmute(base.wrapping_add_signed(r_addend)) };
                    write_val(base, rel.r_offset(), ifunc());
                } else {
                    unreachable!()
                }
            }

            // Prepare for lazy binding if we have PLT relocations
            if !reloc.pltrel.is_empty() {
                prepare_lazy_bind(
                    self.got().unwrap().as_ptr(),
                    Arc::as_ptr(&core.inner) as usize,
                );
            }

            // Ensure either local or global lazy scope is available
            assert!(
                reloc.pltrel.is_empty()
                    || local_lazy_scope.is_some()
                    || GLOBAL_SCOPE.load(Ordering::Relaxed) != 0,
                "neither local lazy scope nor global scope is set"
            );

            // Set the local lazy scope if provided
            if let Some(lazy_scope) = local_lazy_scope {
                core.set_lazy_scope(lazy_scope);
            }
        } else {
            // Without lazy binding, resolve all PLT relocations immediately
            for rel in reloc.pltrel {
                let r_type = rel.r_type() as u32;
                let r_sym = rel.r_symbol();
                let r_addend = rel.r_addend(base);

                // Handle jump slot relocations
                if likely(r_type == REL_JUMP_SLOT) {
                    if let Some(symbol) = find_symbol_addr(pre_find, core, symtab, scope, r_sym) {
                        write_val(base, rel.r_offset(), symbol);
                        continue;
                    }
                } else if unlikely(r_type == REL_IRELATIVE) {
                    // Handle indirect function relocations
                    let ifunc: fn() -> usize =
                        unsafe { core::mem::transmute(base.wrapping_add_signed(r_addend)) };
                    write_val(base, rel.r_offset(), ifunc());
                    continue;
                }

                // Handle unknown relocations with the provided handler
                deal_unknown(rel, core, scope)
                    .map_err(|err| reloc_error(r_type as _, r_sym, err, core))?;
            }

            // Apply RELRO (RELocation Read-Only) protection if available
            if let Some(relro) = self.relro() {
                relro.relro()?;
            }
        }
        Ok(self)
    }

    /// Perform relative relocations (REL_RELATIVE)
    fn relocate_relative(&self) -> &Self {
        let core = self.core_component_ref();
        let reloc = self.relocation();
        let base = core.base();

        match reloc.relative {
            RelativeRel::Rel(rel) => {
                assert!(rel.is_empty() || rel[0].r_type() == REL_RELATIVE as usize);
                // Apply all relative relocations: new_value = base_address + addend
                rel.iter().for_each(|rel| {
                    debug_assert!(rel.r_type() == REL_RELATIVE as usize);
                    let r_addend = rel.r_addend(base);
                    write_val(base, rel.r_offset(), base.wrapping_add_signed(r_addend));
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
    fn relocate_dynrel<F>(
        &self,
        scope: &[&Relocated],
        pre_find: &F,
        deal_unknown: &mut UnknownHandler,
    ) -> Result<&Self>
    where
        F: Fn(&str) -> Option<*const ()>,
    {
        /*
            Relocation formula components:
            A = Addend used to compute the value of the relocatable field
            B = Base address at which a shared object is loaded
            S = Value of the symbol whose index resides in the relocation entry
        */

        let core = self.core_component_ref();
        let reloc = self.relocation();
        let symtab = self.symtab().unwrap();
        let base = core.base();

        // Process each dynamic relocation entry
        for rel in reloc.dynrel {
            let r_type = rel.r_type() as _;
            let r_sym = rel.r_symbol();
            let r_addend = rel.r_addend(base);

            match r_type {
                // Handle GOT and symbolic relocations
                REL_GOT | REL_SYMBOLIC => {
                    if let Some(symbol) = find_symbol_addr(pre_find, core, symtab, scope, r_sym) {
                        write_val(base, rel.r_offset(), symbol);
                        continue;
                    }
                }
                // Handle TLS (Thread Local Storage) offset relocations
                REL_DTPOFF => {
                    if let Some(symdef) = find_symdef(core, scope, r_sym) {
                        // Calculate offset within TLS block
                        let tls_val =
                            (symdef.sym.unwrap().st_value().wrapping_add_signed(r_addend))
                                .wrapping_sub(TLS_DTV_OFFSET);
                        write_val(base, rel.r_offset(), tls_val);
                        continue;
                    }
                }
                // Handle copy relocations (typically for global data)
                REL_COPY => {
                    if let Some(symbol) = find_symdef(core, scope, r_sym) {
                        let len = symbol.sym.unwrap().st_size();
                        let dest = core.segments().get_slice_mut::<u8>(rel.r_offset(), len);
                        let src = core
                            .segments()
                            .get_slice(symbol.sym.unwrap().st_value(), len);
                        dest.copy_from_slice(src);
                        continue;
                    }
                }
                // No relocation needed
                REL_NONE => continue,
                // Unknown relocation type
                _ => {}
            }

            // Handle unknown relocations with the provided handler
            deal_unknown(rel, core, scope)
                .map_err(|err| reloc_error(r_type as _, r_sym, err, core))?;
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
    pub(crate) fn is_empty(&self) -> bool {
        self.relative.is_empty() && self.dynrel.is_empty() && self.pltrel.is_empty()
    }
}
