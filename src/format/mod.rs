pub(crate) mod dylib;
pub(crate) mod exec;

use crate::{
    ELFRelro, ElfRelocation, Loader, Result,
    arch::{Dyn, ElfPhdr, ElfRelType},
    dynamic::ElfDynamic,
    loader::Builder,
    mmap::Mmap,
    object::{ElfObject, ElfObjectAsync},
    parse_dynamic_error,
    relocation::LazyScope,
    segment::ElfSegments,
    symbol::SymbolTable,
};
use alloc::{
    boxed::Box,
    ffi::CString,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::{
    any::Any,
    ffi::{CStr, c_int},
    fmt::Debug,
    marker::PhantomData,
    ops::Deref,
    ptr::{NonNull, null},
    sync::atomic::{AtomicBool, Ordering},
};
use dylib::{ElfDylib, RelocatedDylib};
use elf::abi::PT_LOAD;
use exec::{ElfExec, RelocatedExec};

struct DataItem {
    key: u8,
    value: Option<Box<dyn Any>>,
}

/// User-defined data associated with the loaded ELF file
pub struct UserData {
    data: Vec<DataItem>,
}

impl UserData {
    #[inline]
    pub const fn empty() -> Self {
        Self { data: Vec::new() }
    }

    #[inline]
    pub fn insert(&mut self, key: u8, value: Box<dyn Any>) -> Option<Box<dyn Any>> {
        for item in self.data.iter_mut() {
            if item.key == key {
                let old = core::mem::take(&mut item.value);
                item.value = Some(value);
                return old;
            }
        }
        self.data.push(DataItem {
            key,
            value: Some(value),
        });
        None
    }

    #[inline]
    pub fn get(&self, key: u8) -> Option<&Box<dyn Any>> {
        self.data.iter().find_map(|item| {
            if item.key == key {
                return item.value.as_ref();
            }
            None
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct InitParams {
    pub argc: usize,
    pub argv: usize,
    pub envp: usize,
}

pub(crate) struct ElfInit {
    init_param: Option<InitParams>,
    /// .init
    init_fn: Option<extern "C" fn()>,
    /// .init_array
    init_array_fn: Option<&'static [extern "C" fn()]>,
}

impl ElfInit {
    #[inline]
    pub(crate) fn call_init(self) {
        if let Some(init_params) = self.init_param {
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
    }
}

impl Deref for Relocated<'_> {
    type Target = CoreComponent;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

/// An unrelocated elf file
#[derive(Debug)]
pub enum Elf {
    Dylib(ElfDylib),
    Exec(ElfExec),
}

/// A elf file that has been relocated
#[derive(Debug, Clone)]
pub enum RelocatedElf<'scope> {
    Dylib(RelocatedDylib<'scope>),
    Exec(RelocatedExec<'scope>),
}

impl<'scope> RelocatedElf<'scope> {
    #[inline]
    pub fn into_dylib(self) -> Option<RelocatedDylib<'scope>> {
        match self {
            RelocatedElf::Dylib(dylib) => Some(dylib),
            RelocatedElf::Exec(_) => None,
        }
    }

    #[inline]
    pub fn into_exec(self) -> Option<RelocatedExec<'scope>> {
        match self {
            RelocatedElf::Dylib(_) => None,
            RelocatedElf::Exec(exec) => Some(exec),
        }
    }

    #[inline]
    pub fn as_dylib(&self) -> Option<&RelocatedDylib<'scope>> {
        match self {
            RelocatedElf::Dylib(dylib) => Some(dylib),
            RelocatedElf::Exec(_) => None,
        }
    }
}

impl Deref for Elf {
    type Target = ElfCommonPart;

    fn deref(&self) -> &Self::Target {
        match self {
            Elf::Dylib(elf_dylib) => &elf_dylib,
            Elf::Exec(elf_exec) => &elf_exec,
        }
    }
}

// 使用CoreComponentRef是防止出现循环引用
pub(crate) fn create_lazy_scope<F>(libs: Vec<CoreComponentRef>, pre_find: &F) -> LazyScope
where
    F: Fn(&str) -> Option<*const ()>,
{
    Arc::new(move |name| {
        libs.iter().find_map(|lib| {
            pre_find(name).or_else(|| unsafe {
                RelocatedDylib::from_core_component(lib.upgrade().unwrap())
                    .get::<()>(name)
                    .map(|sym| sym.into_raw())
            })
        })
    })
}

impl Elf {
    /// Relocate the elf file with the given dynamic libraries and function closure.
    /// # Note
    /// During relocation, the symbol is first searched in the function closure `pre_find`.
    pub fn easy_relocate<'iter, 'scope, 'find, 'lib, S, F>(
        self,
        scope: S,
        pre_find: &'find F,
    ) -> Result<RelocatedElf<'lib>>
    where
        S: Iterator<Item = &'iter RelocatedDylib<'scope>> + Clone,
        F: Fn(&str) -> Option<*const ()>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        match self {
            Elf::Dylib(elf_dylib) => Ok(RelocatedElf::Dylib(
                elf_dylib.easy_relocate(scope, pre_find)?,
            )),
            Elf::Exec(elf_exec) => Ok(RelocatedElf::Exec(elf_exec.easy_relocate(scope, pre_find)?)),
        }
    }

    /// Relocate the elf file with the given dynamic libraries and function closure.
    /// # Note
    /// * During relocation, the symbol is first searched in the function closure `pre_find`.
    /// * The `deal_unknown` function is used to handle relocation types not implemented by efl_loader or failed relocations
    /// * relocation will be done in the exact order in which the dynamic libraries appear in `scope`.
    /// * When lazy binding, the symbol is first looked for in the global scope and then in the local lazy scope
    pub fn relocate<'iter, 'scope, 'find, 'lib, S, F, D>(
        self,
        scope: S,
        pre_find: &'find F,
        deal_unknown: D,
        local_lazy_scope: Option<LazyScope<'lib>>,
    ) -> Result<RelocatedElf<'lib>>
    where
        S: Iterator<Item = &'iter RelocatedDylib<'scope>> + Clone,
        F: Fn(&str) -> Option<*const ()>,
        D: Fn(&ElfRelType, &CoreComponent, S) -> core::result::Result<(), Box<dyn Any>>,
        'scope: 'iter,
        'iter: 'lib,
        'find: 'lib,
    {
        let relocated_elf = match self {
            Elf::Dylib(elf_dylib) => RelocatedElf::Dylib(elf_dylib.relocate(
                scope,
                pre_find,
                deal_unknown,
                local_lazy_scope,
            )?),
            Elf::Exec(elf_exec) => RelocatedElf::Exec(elf_exec.relocate(
                scope,
                pre_find,
                deal_unknown,
                local_lazy_scope,
            )?),
        };
        Ok(relocated_elf)
    }
}

#[derive(Clone)]
pub(crate) struct Relocated<'scope> {
    pub(crate) core: CoreComponent,
    pub(crate) _marker: PhantomData<&'scope ()>,
}

pub(crate) struct CoreComponentInner {
    /// is initialized
    is_init: AtomicBool,
    /// file name
    name: CString,
    /// elf symbols
    pub(crate) symbols: Option<SymbolTable>,
    /// dynamic
    dynamic: Option<NonNull<Dyn>>,
    /// rela.plt
    pub(crate) pltrel: Option<NonNull<ElfRelType>>,
    /// phdrs
    phdrs: &'static [ElfPhdr],
    /// .fini
    fini_fn: Option<extern "C" fn()>,
    /// .fini_array
    fini_array_fn: Option<&'static [extern "C" fn()]>,
    /// needed libs' name
    needed_libs: Box<[&'static str]>,
    /// user data
    user_data: UserData,
    /// lazy binding scope
    pub(crate) lazy_scope: Option<LazyScope<'static>>,
    /// semgents
    pub(crate) segments: ElfSegments,
}

impl Drop for CoreComponentInner {
    fn drop(&mut self) {
        if self.is_init.load(Ordering::Relaxed) {
            self.fini_fn
                .iter()
                .chain(self.fini_array_fn.unwrap_or(&[]).iter())
                .for_each(|fini| fini());
        }
    }
}

/// `CoreComponentRef` is a version of `CoreComponent` that holds a non-owning reference to the managed allocation.
pub struct CoreComponentRef {
    inner: Weak<CoreComponentInner>,
}

impl CoreComponentRef {
    /// Attempts to upgrade the Weak pointer to an Arc
    pub fn upgrade(&self) -> Option<CoreComponent> {
        self.inner.upgrade().map(|inner| CoreComponent { inner })
    }
}

/// The core part of an elf object
#[derive(Clone)]
pub struct CoreComponent {
    pub(crate) inner: Arc<CoreComponentInner>,
}

unsafe impl Sync for CoreComponent {}
unsafe impl Send for CoreComponent {}

impl CoreComponent {
    #[inline]
    pub(crate) fn set_lazy_scope(&self, lazy_scope: Option<LazyScope>) {
        // 因为在完成重定位前，只有unsafe的方法可以拿到CoreComponent的引用，所以这里认为是安全的
        unsafe {
            let ptr = &mut *(Arc::as_ptr(&self.inner) as *mut CoreComponentInner);
            // 在relocate接口处保证了lazy_scope的声明周期，因此这里直接转换
            ptr.lazy_scope = core::mem::transmute(lazy_scope);
        };
    }

    #[inline]
    pub(crate) fn set_init(&self) {
        self.inner.is_init.store(true, Ordering::Relaxed);
    }

    #[inline]
    /// Creates a new Weak pointer to this allocation.
    pub fn downgrade(&self) -> CoreComponentRef {
        CoreComponentRef {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// Gets user data from the elf object.
    #[inline]
    pub fn user_data(&self) -> &UserData {
        &self.inner.user_data
    }

    /// Gets the number of strong references to the elf object.
    #[inline]
    pub fn strong_count(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    /// Gets the number of weak references to the elf object.
    #[inline]
    pub fn weak_count(&self) -> usize {
        Arc::weak_count(&self.inner)
    }

    /// Gets the name of the elf object.
    #[inline]
    pub fn name(&self) -> &str {
        self.inner.name.to_str().unwrap()
    }

    /// Gets the C-style name of the elf object.
    #[inline]
    pub fn cname(&self) -> &CStr {
        &self.inner.name
    }

    /// Gets the short name of the elf object.
    #[inline]
    pub fn shortname(&self) -> &str {
        self.name().split('/').last().unwrap()
    }

    /// Gets the base address of the elf object.
    #[inline]
    pub fn base(&self) -> usize {
        self.inner.segments.base()
    }

    /// Gets the memory length of the elf object map.
    #[inline]
    pub fn map_len(&self) -> usize {
        self.inner.segments.len()
    }

    /// Gets the program headers of the elf object.
    #[inline]
    pub fn phdrs(&self) -> &[ElfPhdr] {
        &self.inner.phdrs
    }

    /// Gets the address of the dynamic section.
    #[inline]
    pub fn dynamic(&self) -> Option<NonNull<Dyn>> {
        self.inner.dynamic
    }

    /// Gets the needed libs' name of the elf object.
    #[inline]
    pub fn needed_libs(&self) -> &[&str] {
        &self.inner.needed_libs
    }

    /// Gets the symbol table.
    #[inline]
    pub fn symtab(&self) -> Option<&SymbolTable> {
        self.inner.symbols.as_ref()
    }

    #[inline]
    pub(crate) fn segments(&self) -> &ElfSegments {
        &self.inner.segments
    }

    fn from_raw(
        name: CString,
        base: usize,
        dynamic: ElfDynamic,
        phdrs: &'static [ElfPhdr],
        mut segments: ElfSegments,
        user_data: UserData,
    ) -> Self {
        segments.offset = (segments.memory.as_ptr() as usize).wrapping_sub(base);
        Self {
            inner: Arc::new(CoreComponentInner {
                name,
                is_init: AtomicBool::new(true),
                symbols: Some(SymbolTable::new(&dynamic)),
                pltrel: None,
                dynamic: NonNull::new(dynamic.dyn_ptr as _),
                phdrs,
                segments,
                fini_fn: None,
                fini_array_fn: None,
                needed_libs: Box::new([]),
                user_data,
                lazy_scope: None,
            }),
        }
    }
}

impl Debug for CoreComponent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dylib")
            .field("name", &self.inner.name)
            .finish()
    }
}

impl Deref for ElfCommonPart {
    type Target = CoreComponent;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

pub struct ElfCommonPart {
    /// entry
    entry: usize,
    /// .got.plt
    pub(crate) got: Option<NonNull<usize>>,
    /// rela.dyn and rela.plt
    pub(crate) relocation: ElfRelocation,
    /// GNU_RELRO segment
    pub(crate) relro: Option<ELFRelro>,
    /// init
    pub(crate) init: ElfInit,
    /// lazy binding
    lazy: bool,
    /// DT_RPATH
    rpath: Option<&'static str>,
    /// DT_RUNPATH
    runpath: Option<&'static str>,
    /// PT_INTERP
    interp: Option<&'static str>,
    /// core component
    pub(crate) core: CoreComponent,
}

impl ElfCommonPart {
    /// Gets the entry point of the elf object.
    #[inline]
    pub fn entry(&self) -> usize {
        self.entry + self.base()
    }

    /// Gets the core component reference of the elf object
    #[inline]
    pub fn core_component_ref(&self) -> &CoreComponent {
        &self.core
    }

    /// Gets the core component of the elf object
    #[inline]
    pub fn core_component(&self) -> CoreComponent {
        self.core.clone()
    }

    /// Whether lazy binding is enabled for the current elf object.
    #[inline]
    pub fn is_lazy(&self) -> bool {
        self.lazy
    }

    /// Gets the DT_RPATH value.
    #[inline]
    pub fn rpath(&self) -> Option<&str> {
        self.rpath
    }

    /// Gets the DT_RUNPATH value.
    #[inline]
    pub fn runpath(&self) -> Option<&str> {
        self.runpath
    }

    /// Gets the PT_INTERP value.
    #[inline]
    pub fn interp(&self) -> Option<&str> {
        self.interp
    }
}

impl Builder {
    pub(crate) fn create_common(self, phdrs: &[ElfPhdr], is_dylib: bool) -> Result<ElfCommonPart> {
        let common = if let Some(dynamic) = self.dynamic {
            let (phdr_start, phdr_end) = self.ehdr.phdr_range();
            // 获取映射到内存中的Phdr
            let phdrs = self.phdr_mmap.unwrap_or_else(|| {
                phdrs
                    .iter()
                    .filter(|phdr| phdr.p_type == PT_LOAD)
                    .find_map(|phdr| {
                        let cur_range =
                            phdr.p_offset as usize..(phdr.p_offset + phdr.p_filesz) as usize;
                        if cur_range.contains(&phdr_start) && cur_range.contains(&phdr_end) {
                            return unsafe {
                                Some(core::mem::transmute(self.segments.get_slice::<ElfPhdr>(
                                    phdr.p_vaddr as usize + phdr_start - cur_range.start,
                                    self.ehdr.e_phnum() * size_of::<ElfPhdr>(),
                                )))
                            };
                        }
                        None
                    })
                    .unwrap()
            });

            let relocation = ElfRelocation::new(dynamic.pltrel, dynamic.dynrel, dynamic.rela_count);
            let symbols = SymbolTable::new(&dynamic);
            let needed_libs: Vec<&'static str> = dynamic
                .needed_libs
                .iter()
                .map(|needed_lib| symbols.strtab().get_str(needed_lib.get()))
                .collect();
            ElfCommonPart {
                entry: self.ehdr.e_entry as usize,
                relro: self.relro,
                relocation,
                init: ElfInit {
                    init_param: self.init_params,
                    init_fn: dynamic.init_fn,
                    init_array_fn: dynamic.init_array_fn,
                },
                interp: self.interp,
                lazy: self.lazy_bind.unwrap_or(!dynamic.bind_now),
                got: dynamic.got,
                rpath: dynamic
                    .rpath_off
                    .map(|rpath_off| symbols.strtab().get_str(rpath_off.get())),
                runpath: dynamic
                    .runpath_off
                    .map(|runpath_off| symbols.strtab().get_str(runpath_off.get())),
                core: CoreComponent {
                    inner: Arc::new(CoreComponentInner {
                        is_init: AtomicBool::new(false),
                        name: self.name,
                        symbols: Some(symbols),
                        dynamic: NonNull::new(dynamic.dyn_ptr as _),
                        pltrel: NonNull::new(dynamic.pltrel.map_or(null(), |plt| plt.as_ptr()) as _),
                        phdrs,
                        fini_fn: dynamic.fini_fn,
                        fini_array_fn: dynamic.fini_array_fn,
                        segments: self.segments,
                        needed_libs: needed_libs.into_boxed_slice(),
                        user_data: self.user_data,
                        lazy_scope: None,
                    }),
                },
            }
        } else {
            if is_dylib {
                return Err(parse_dynamic_error("dylib does not have dynamic"));
            }
            let relocation = ElfRelocation::new(None, None, None);
            ElfCommonPart {
                entry: self.ehdr.e_entry as usize,
                relro: self.relro,
                relocation,
                init: ElfInit {
                    init_param: self.init_params,
                    init_fn: None,
                    init_array_fn: None,
                },
                interp: self.interp,
                lazy: self.lazy_bind.unwrap_or(false),
                got: None,
                rpath: None,
                runpath: None,
                core: CoreComponent {
                    inner: Arc::new(CoreComponentInner {
                        is_init: AtomicBool::new(false),
                        name: self.name,
                        symbols: None,
                        dynamic: None,
                        pltrel: None,
                        phdrs: &[],
                        fini_fn: None,
                        fini_array_fn: None,
                        segments: self.segments,
                        needed_libs: Box::new([]),
                        user_data: self.user_data,
                        lazy_scope: None,
                    }),
                },
            }
        };
        Ok(common)
    }

    pub(crate) fn create_elf(self, phdrs: &[ElfPhdr], is_dylib: bool) -> Result<Elf> {
        let elf = if is_dylib {
            Elf::Dylib(self.create_dylib(phdrs)?)
        } else {
            Elf::Exec(self.create_exec(phdrs)?)
        };
        Ok(elf)
    }
}

impl<M: Mmap> Loader<M> {
    /// Load a elf file into memory
    pub fn easy_load(&mut self, object: impl ElfObject) -> Result<Elf> {
        self.load(object, None)
    }

    /// Load a elf file into memory
    /// # Note
    /// * When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    pub fn load(&mut self, mut object: impl ElfObject, lazy_bind: Option<bool>) -> Result<Elf> {
        let ehdr = self.buf.prepare_ehdr(&mut object)?;
        let is_dylib = ehdr.is_dylib();
        let (builder, phdrs) = self.load_impl(ehdr, object, lazy_bind)?;
        builder.create_elf(phdrs, is_dylib)
    }

    /// Load a elf file into memory
    /// # Note
    /// * When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    pub async fn load_async(
        &mut self,
        mut object: impl ElfObjectAsync,
        lazy_bind: Option<bool>,
    ) -> Result<Elf> {
        let ehdr = self.buf.prepare_ehdr(&mut object)?;
        let is_dylib = ehdr.is_dylib();
        let (builder, phdrs) = self.load_async_impl(ehdr, object, lazy_bind).await?;
        builder.create_elf(phdrs, is_dylib)
    }
}
