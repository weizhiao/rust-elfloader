use crate::{
    CoreComponent, Result, UserData,
    arch::{Dyn, ElfPhdr},
    dynamic::ElfDynamic,
    ehdr::ElfHeader,
    format::{CoreComponentInner, ElfPhdrs},
    loader::{FnHandler, Hook},
    mmap::Mmap,
    parse_phdr_error,
    relocation::dynamic_link::DynamicRelocation,
    segment::{ELFRelro, ElfSegments},
    symbol::SymbolTable,
};
use alloc::{boxed::Box, ffi::CString, format, vec::Vec};
use core::{
    cell::Cell,
    ffi::{CStr, c_char},
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::{NonNull, null},
    sync::atomic::AtomicBool,
};
use delegate::delegate;
use elf::abi::PT_LOAD;
use elf::abi::{PT_DYNAMIC, PT_GNU_RELRO, PT_INTERP, PT_PHDR};

pub use dylib::{ElfDylib, RelocatedDylib};
pub use exec::{ElfExec, RelocatedExec};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::Arc;
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::{Arc, Weak};

pub(crate) mod dylib;
pub(crate) mod exec;

struct ElfExtraData {
    /// lazy binding
    lazy: bool,
    /// .got.plt
    got: Option<NonNull<usize>>,
    /// rela.dyn and rela.plt
    relocation: DynamicRelocation,
    /// GNU_RELRO segment
    relro: Option<ELFRelro>,
    /// init
    init: Box<dyn Fn()>,
    /// DT_RPATH
    rpath: Option<&'static str>,
    /// DT_RUNPATH
    runpath: Option<&'static str>,
}

struct LazyData {
    /// core component
    core: CoreComponent,
    /// extra data
    extra: ElfExtraData,
}

enum State {
    Empty,
    Uninit {
        is_dylib: bool,
        phdrs: ElfPhdrs,
        init_handler: FnHandler,
        fini_handler: FnHandler,
        name: CString,
        dynamic_ptr: Option<NonNull<Dyn>>,
        segments: ElfSegments,
        relro: Option<ELFRelro>,
        user_data: UserData,
        lazy_bind: Option<bool>,
    },
    Init(LazyData),
}

impl State {
    fn init(self) -> Self {
        let lazy_data = match self {
            State::Uninit {
                name,
                dynamic_ptr,
                segments,
                relro,
                user_data,
                lazy_bind,
                init_handler,
                fini_handler,
                phdrs,
                is_dylib,
            } => {
                if let Some(dynamic_ptr) = dynamic_ptr {
                    let dynamic = ElfDynamic::new(dynamic_ptr.as_ptr(), &segments).unwrap();
                    let relocation = DynamicRelocation::new(
                        dynamic.pltrel,
                        dynamic.dynrel,
                        dynamic.relr,
                        dynamic.rel_count,
                    );
                    let symbols = SymbolTable::from_dynamic(&dynamic);
                    let needed_libs: Vec<&'static str> = dynamic
                        .needed_libs
                        .iter()
                        .map(|needed_lib| symbols.strtab().get_str(needed_lib.get()))
                        .collect();
                    LazyData {
                        extra: ElfExtraData {
                            lazy: lazy_bind.unwrap_or(!dynamic.bind_now),
                            relro,
                            relocation,
                            init: Box::new(move || {
                                init_handler(dynamic.init_fn, dynamic.init_array_fn)
                            }),
                            got: dynamic.got,
                            rpath: dynamic
                                .rpath_off
                                .map(|rpath_off| symbols.strtab().get_str(rpath_off.get())),
                            runpath: dynamic
                                .runpath_off
                                .map(|runpath_off| symbols.strtab().get_str(runpath_off.get())),
                        },
                        core: CoreComponent {
                            inner: Arc::new(CoreComponentInner {
                                is_init: AtomicBool::new(false),
                                name,
                                symbols: Some(symbols),
                                dynamic: NonNull::new(dynamic.dyn_ptr as _),
                                pltrel: NonNull::new(
                                    dynamic.pltrel.map_or(null(), |plt| plt.as_ptr()) as _,
                                ),
                                phdrs,
                                fini: dynamic.fini_fn,
                                fini_array: dynamic.fini_array_fn,
                                fini_handler,
                                segments,
                                needed_libs: needed_libs.into_boxed_slice(),
                                user_data,
                                lazy_scope: None,
                            }),
                        },
                    }
                } else {
                    assert!(!is_dylib, "dylib does not have dynamic");
                    let relocation = DynamicRelocation::new(None, None, None, None);
                    LazyData {
                        core: CoreComponent {
                            inner: Arc::new(CoreComponentInner {
                                is_init: AtomicBool::new(false),
                                name,
                                symbols: None,
                                dynamic: None,
                                pltrel: None,
                                phdrs: ElfPhdrs::Mmap(&[]),
                                fini: None,
                                fini_array: None,
                                fini_handler: Arc::new(|_, _| {}),
                                segments,
                                needed_libs: Box::new([]),
                                user_data,
                                lazy_scope: None,
                            }),
                        },
                        extra: ElfExtraData {
                            lazy: lazy_bind.unwrap_or(false),
                            relro,
                            relocation,
                            init: Box::new(|| {}),
                            got: None,
                            rpath: None,
                            runpath: None,
                        },
                    }
                }
            }
            State::Empty | State::Init(_) => unreachable!(),
        };
        State::Init(lazy_data)
    }
}

struct LazyParse {
    state: Cell<State>,
}

impl LazyParse {
    fn force(&self) -> &LazyData {
        // 快路径加速
        if let State::Init(lazy_data) = unsafe { &*self.state.as_ptr() } {
            return lazy_data;
        }
        self.state.set(self.state.replace(State::Empty).init());
        match unsafe { &*self.state.as_ptr() } {
            State::Empty | State::Uninit { .. } => unreachable!(),
            State::Init(lazy_data) => lazy_data,
        }
    }

    fn force_mut(&mut self) -> &mut LazyData {
        // 快路径加速
        if let State::Init(lazy_data) = self.state.get_mut() {
            return unsafe { core::mem::transmute(lazy_data) };
        }
        self.state.set(self.state.replace(State::Empty).init());
        match unsafe { &mut *self.state.as_ptr() } {
            State::Empty | State::Uninit { .. } => unreachable!(),
            State::Init(lazy_data) => lazy_data,
        }
    }
}

impl Deref for LazyParse {
    type Target = LazyData;

    #[inline]
    fn deref(&self) -> &LazyData {
        self.force()
    }
}

impl DerefMut for LazyParse {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.force_mut()
    }
}

/// A common part of elf object
pub struct RelocatedCommonPart {
    /// entry
    entry: usize,
    /// PT_INTERP
    interp: Option<&'static str>,
    /// file name
    name: &'static CStr,
    /// phdrs
    phdrs: ElfPhdrs,
    /// data parse lazy
    data: LazyParse,
}

impl RelocatedCommonPart {
    /// Gets the entry point of the elf object.
    #[inline]
    pub fn entry(&self) -> usize {
        self.entry
    }

    /// Gets the core component reference of the elf object
    #[inline]
    pub fn core_component_ref(&self) -> &CoreComponent {
        &self.data.core
    }

    /// Gets the core component of the elf object
    #[inline]
    pub fn core_component(&self) -> CoreComponent {
        self.data.core.clone()
    }

    #[inline]
    /// Gets the core component of the elf object
    pub fn into_core_component(self) -> CoreComponent {
        self.data.force();
        match self.data.state.into_inner() {
            State::Empty | State::Uninit { .. } => unreachable!(),
            State::Init(lazy_data) => lazy_data.core,
        }
    }

    /// Whether lazy binding is enabled for the current elf object.
    #[inline]
    pub fn is_lazy(&self) -> bool {
        self.data.extra.lazy
    }

    /// Gets the DT_RPATH value.
    #[inline]
    pub fn rpath(&self) -> Option<&str> {
        self.data.extra.rpath
    }

    /// Gets the DT_RUNPATH value.
    #[inline]
    pub fn runpath(&self) -> Option<&str> {
        self.data.extra.runpath
    }

    /// Gets the PT_INTERP value.
    #[inline]
    pub fn interp(&self) -> Option<&str> {
        self.interp
    }

    /// Gets the name of the elf object.
    #[inline]
    pub fn name(&self) -> &str {
        self.name.to_str().unwrap()
    }

    /// Gets the C-style name of the elf object.
    #[inline]
    pub fn cname(&self) -> &CStr {
        self.name
    }

    /// Gets the short name of the elf object.
    #[inline]
    pub fn shortname(&self) -> &str {
        self.name().split('/').next_back().unwrap()
    }

    /// Gets the program headers of the elf object.
    pub fn phdrs(&self) -> &[ElfPhdr] {
        match &self.phdrs {
            ElfPhdrs::Mmap(phdrs) => &phdrs,
            ElfPhdrs::Vec(phdrs) => &phdrs,
        }
    }

    #[inline]
    pub(crate) fn got(&self) -> Option<NonNull<usize>> {
        self.data.extra.got
    }

    #[inline]
    pub(crate) fn relocation(&self) -> &DynamicRelocation {
        &self.data.extra.relocation
    }

    #[inline]
    pub(crate) fn finish(&self) {
        self.data.core.set_init();
        (self.data.extra.init)();
    }

    #[inline]
    pub(crate) fn relro(&self) -> Option<&ELFRelro> {
        self.data.extra.relro.as_ref()
    }

    #[inline]
    pub(crate) fn user_data_mut(&mut self) -> Option<&mut UserData> {
        Arc::get_mut(&mut self.data.core.inner).map(|inner| &mut inner.user_data)
    }

    delegate! {
        to self.data.core{
            pub(crate) fn symtab(&self) -> Option<&SymbolTable>;
            /// Gets the base address of the elf object.
            pub fn base(&self) -> usize;
            /// Gets the needed libs' name of the elf object.
            pub fn needed_libs(&self) -> &[&str];
            /// Gets the address of the dynamic section.
            pub fn dynamic(&self) -> Option<NonNull<Dyn>>;
            /// Gets the memory length of the elf object map.
            pub fn map_len(&self) -> usize;
            /// Gets user data from the elf object.
            pub fn user_data(&self) -> &UserData;
        }
    }
}

pub(crate) struct RelocatedBuilder<'hook, M: Mmap> {
    hook: Option<&'hook Hook>,
    phdr_mmap: Option<&'static [ElfPhdr]>,
    name: CString,
    lazy_bind: Option<bool>,
    ehdr: ElfHeader,
    relro: Option<ELFRelro>,
    dynamic_ptr: Option<NonNull<Dyn>>,
    user_data: UserData,
    segments: ElfSegments,
    init_fn: FnHandler,
    fini_fn: FnHandler,
    interp: Option<NonNull<c_char>>,
    _marker: PhantomData<M>,
}

impl<'hook, M: Mmap> RelocatedBuilder<'hook, M> {
    pub(crate) const fn new(
        hook: Option<&'hook Hook>,
        segments: ElfSegments,
        name: CString,
        lazy_bind: Option<bool>,
        ehdr: ElfHeader,
        init_fn: FnHandler,
        fini_fn: FnHandler,
    ) -> Self {
        Self {
            hook,
            phdr_mmap: None,
            name,
            lazy_bind,
            ehdr,
            relro: None,
            dynamic_ptr: None,
            segments,
            user_data: UserData::empty(),
            init_fn,
            fini_fn,
            interp: None,
            _marker: PhantomData,
        }
    }
    fn parse_phdr(&mut self, phdr: &ElfPhdr) -> Result<()> {
        if let Some(hook) = self.hook {
            hook(&self.name, phdr, &self.segments, &mut self.user_data).map_err(|err| {
                parse_phdr_error(
                    format!(
                        "failed to execute the hook function on dylib: {}",
                        self.name.to_str().unwrap()
                    ),
                    err,
                )
            })?;
        }
        match phdr.p_type {
            // 解析.dynamic section
            PT_DYNAMIC => {
                self.dynamic_ptr =
                    Some(NonNull::new(self.segments.get_mut_ptr(phdr.p_paddr as usize)).unwrap())
            }
            PT_GNU_RELRO => self.relro = Some(ELFRelro::new::<M>(phdr, self.segments.base())),
            PT_PHDR => {
                self.phdr_mmap = Some(
                    self.segments
                        .get_slice::<ElfPhdr>(phdr.p_vaddr as usize, phdr.p_memsz as usize),
                );
            }
            PT_INTERP => {
                self.interp =
                    Some(NonNull::new(self.segments.get_mut_ptr(phdr.p_vaddr as usize)).unwrap());
            }
            _ => {}
        };
        Ok(())
    }

    fn create_phdrs(&self, phdrs: &[ElfPhdr]) -> ElfPhdrs {
        let (phdr_start, phdr_end) = self.ehdr.phdr_range();
        // 获取映射到内存中的Phdr
        self.phdr_mmap
            .or_else(|| {
                phdrs
                    .iter()
                    .filter(|phdr| phdr.p_type == PT_LOAD)
                    .find_map(|phdr| {
                        let cur_range =
                            phdr.p_offset as usize..(phdr.p_offset + phdr.p_filesz) as usize;
                        if cur_range.contains(&phdr_start) && cur_range.contains(&phdr_end) {
                            return Some(self.segments.get_slice::<ElfPhdr>(
                                phdr.p_vaddr as usize + phdr_start - cur_range.start,
                                self.ehdr.e_phnum() * size_of::<ElfPhdr>(),
                            ));
                        }
                        None
                    })
            })
            .map(|phdrs| ElfPhdrs::Mmap(phdrs))
            .unwrap_or_else(|| ElfPhdrs::Vec(Vec::from(phdrs)))
    }

    pub(crate) fn build(mut self, phdrs: &[ElfPhdr]) -> Result<RelocatedCommonPart> {
        let is_dylib = self.ehdr.is_dylib();
        for phdr in phdrs {
            self.parse_phdr(phdr)?;
        }
        let phdrs = self.create_phdrs(phdrs);
        Ok(RelocatedCommonPart {
            entry: self.ehdr.e_entry as usize + if is_dylib { self.segments.base() } else { 0 },
            interp: self
                .interp
                .map(|s| unsafe { CStr::from_ptr(s.as_ptr()).to_str().unwrap() }),
            name: unsafe { core::mem::transmute::<&CStr, &CStr>(self.name.as_c_str()) },
            phdrs: phdrs.clone(),
            data: LazyParse {
                state: Cell::new(State::Uninit {
                    is_dylib,
                    phdrs,
                    init_handler: self.init_fn,
                    fini_handler: self.fini_fn,
                    name: self.name,
                    dynamic_ptr: self.dynamic_ptr,
                    segments: self.segments,
                    relro: self.relro,
                    user_data: self.user_data,
                    lazy_bind: self.lazy_bind,
                }),
            },
        })
    }
}
