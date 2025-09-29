pub(crate) mod relocatable;
pub(crate) mod relocated;

use crate::{
    Loader, Result,
    arch::{Dyn, ElfPhdr, ElfRelType},
    dynamic::ElfDynamic,
    format::relocated::{RelocatedCommonPart, ElfDylib, ElfExec, RelocatedDylib, RelocatedExec},
    loader::FnHandler,
    mmap::Mmap,
    object::{ElfObject, ElfObjectAsync},
    relocation::dynamic_link::{LazyScope, UnknownHandler},
    segment::ElfSegments,
    symbol::SymbolTable,
};
use alloc::{boxed::Box, ffi::CString, vec::Vec};
use core::{
    any::Any,
    ffi::CStr,
    fmt::Debug,
    marker::PhantomData,
    ops::Deref,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(not(feature = "portable-atomic"))]
use alloc::sync::{Arc, Weak};
#[cfg(feature = "portable-atomic")]
use portable_atomic_util::{Arc, Weak};

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
        for item in &mut self.data {
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
    type Target = RelocatedCommonPart;

    fn deref(&self) -> &Self::Target {
        match self {
            Elf::Dylib(elf_dylib) => elf_dylib,
            Elf::Exec(elf_exec) => elf_exec,
        }
    }
}

// 使用CoreComponentRef是防止出现循环引用
pub(crate) fn create_lazy_scope<F>(libs: Vec<CoreComponentRef>, pre_find: &'_ F) -> LazyScope<'_>
where
    F: Fn(&str) -> Option<*const ()>,
{
    #[cfg(not(feature = "portable-atomic"))]
    type Ptr<T> = Arc<T>;
    #[cfg(feature = "portable-atomic")]
    type Ptr<T> = Box<T>;
    // workaround unstable CoerceUnsized by create Box<dyn _> then convert using Arc::from
    // https://github.com/rust-lang/rust/issues/18598
    let closure: Ptr<dyn for<'a> Fn(&'a str) -> Option<*const ()>> = Ptr::new(move |name| {
        libs.iter().find_map(|lib| {
            pre_find(name).or_else(|| unsafe {
                RelocatedDylib::from_core_component(lib.upgrade().unwrap())
                    .get::<()>(name)
                    .map(|sym| sym.into_raw())
            })
        })
    });
    closure.into()
}

impl Elf {
    /// Relocate the elf file with the given dynamic libraries and function closure.
    /// # Note
    /// During relocation, the symbol is first searched in the function closure `pre_find`.
    pub fn easy_relocate<'iter, 'scope, 'find, 'lib, F, T>(
        self,
        scope: impl IntoIterator<Item = &'iter T>,
        pre_find: &'find F,
    ) -> Result<RelocatedElf<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
        T: AsRef<Relocated<'scope>> + 'scope,
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
    pub fn relocate<'iter, 'scope, 'find, 'lib, F>(
        self,
        scope: impl AsRef<[&'iter Relocated<'scope>]>,
        pre_find: &'find F,
        deal_unknown: &mut UnknownHandler,
        local_lazy_scope: Option<LazyScope<'lib>>,
    ) -> Result<RelocatedElf<'lib>>
    where
        F: Fn(&str) -> Option<*const ()>,
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
pub struct Relocated<'scope> {
    pub(crate) core: CoreComponent,
    pub(crate) _marker: PhantomData<&'scope ()>,
}

impl Relocated<'_> {
    /// Gets the symbol table.
    pub fn symtab(&self) -> &SymbolTable {
        unsafe { self.core.symtab().unwrap_unchecked() }
    }
}

#[derive(Clone)]
enum ElfPhdrs {
    Mmap(&'static [ElfPhdr]),
    Vec(Vec<ElfPhdr>),
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
    #[allow(unused)]
    pub(crate) pltrel: Option<NonNull<ElfRelType>>,
    /// phdrs
    phdrs: ElfPhdrs,
    /// .fini
    fini: Option<fn()>,
    /// .fini_array
    fini_array: Option<&'static [fn()]>,
    /// custom fini
    fini_handler: FnHandler,
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
            (self.fini_handler)(self.fini, self.fini_array);
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

unsafe impl Sync for CoreComponentInner {}
unsafe impl Send for CoreComponentInner {}

impl CoreComponent {
    #[inline]
    pub(crate) fn set_lazy_scope(&self, lazy_scope: LazyScope) {
        // 因为在完成重定位前，只有unsafe的方法可以拿到CoreComponent的引用，所以这里认为是安全的
        unsafe {
            let ptr = &mut *(Arc::as_ptr(&self.inner) as *mut CoreComponentInner);
            // 在relocate接口处保证了lazy_scope的声明周期，因此这里直接转换
            ptr.lazy_scope = Some(core::mem::transmute::<LazyScope<'_>, LazyScope<'static>>(
                lazy_scope,
            ));
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
        self.name().split('/').next_back().unwrap()
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
        match &self.inner.phdrs {
            ElfPhdrs::Mmap(phdrs) => &phdrs,
            ElfPhdrs::Vec(phdrs) => &phdrs,
        }
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
                symbols: Some(SymbolTable::from_dynamic(&dynamic)),
                pltrel: None,
                dynamic: NonNull::new(dynamic.dyn_ptr as _),
                phdrs: ElfPhdrs::Mmap(phdrs),
                segments,
                fini: None,
                fini_array: None,
                fini_handler: Arc::new(|_, _| {}),
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
        if is_dylib {
            Ok(Elf::Dylib(self.load_dylib(object, lazy_bind)?))
        } else {
            Ok(Elf::Exec(self.load_exec(object, lazy_bind)?))
        }
    }

    // /// Load a elf file into memory
    // /// # Note
    // /// * When `lazy_bind` is not set, lazy binding is enabled using the dynamic library's DT_FLAGS flag.
    // pub async fn load_async(
    //     &mut self,
    //     mut object: impl ElfObjectAsync,
    //     lazy_bind: Option<bool>,
    // ) -> Result<Elf> {
    //     let ehdr = self.buf.prepare_ehdr(&mut object)?;
    //     let is_dylib = ehdr.is_dylib();
    //     let (builder, phdrs) = self.load_async_impl(ehdr, object, lazy_bind).await?;
    //     Ok(builder.create_elf(phdrs, is_dylib))
    // }
}
