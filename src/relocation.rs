use crate::{
    arch::*,
    dynamic::ElfDynamic,
    find_symbol_error, relocate_error,
    segment::ElfSegments,
    symbol::{SymbolData, SymbolInfo},
    ElfDylib, Result, ThreadLocal, Unwind, UserData,
};
use alloc::{boxed::Box, ffi::CString, fmt::Debug, format, string::String, sync::Arc, vec::Vec};
use core::{any::Any, ffi::CStr, marker::PhantomData, ops};
use elf::abi::*;

#[allow(unused)]
struct ElfTls {
    id: usize,
    data: Box<dyn Any>,
}

#[allow(unused)]
pub(crate) struct RelocatedInner {
    name: CString,
    base: usize,
    symbols: SymbolData,
    dynamic: *const Dyn,
    #[cfg(feature = "tls")]
    tls: Option<ElfTls>,
    unwind: Option<Box<dyn Any>>,
    /// semgents
    segments: ElfSegments,
    /// .fini
    fini_fn: Option<extern "C" fn()>,
    /// .fini_array
    fini_array_fn: Option<&'static [extern "C" fn()]>,
    /// user data
    user_data: UserData,
    /// dependency libraries
    dep_libs: Vec<RelocatedDylib>,
}

impl Debug for RelocatedInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RelocatedLibrary")
            .field("name", &self.name)
            .field("base", &self.base)
            .finish()
    }
}

#[derive(Clone)]
pub struct RelocatedDylib {
    pub(crate) inner: Arc<RelocatedInner>,
}

impl Debug for RelocatedDylib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

unsafe impl Send for RelocatedDylib {}
unsafe impl Sync for RelocatedDylib {}

impl RelocatedDylib {
    /// Retrieves the list of dependent libraries.
    ///
    /// This method returns an optional reference to a vector of `RelocatedDylib` instances,
    /// which represent the libraries that the current dynamic library depends on.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// if let Some(dependencies) = library.dep_libs() {
    ///     for lib in dependencies {
    ///         println!("Dependency: {:?}", lib);
    ///     }
    /// } else {
    ///     println!("No dependencies found.");
    /// }
    /// ```
    pub fn dep_libs(&self) -> Option<&Vec<RelocatedDylib>> {
        if self.inner.dep_libs.is_empty() {
            None
        } else {
            Some(&self.inner.dep_libs)
        }
    }

    /// Retrieves the name of the dynamic library.
    ///
    /// This method returns a string slice that represents the name of the dynamic library.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let library_name = library.name();
    /// println!("The dynamic library name is: {}", library_name);
    /// ```
    #[inline]
    pub fn name(&self) -> &str {
        self.inner.name.to_str().unwrap()
    }

    #[inline]
    pub fn cname(&self) -> &CStr {
        &self.inner.name
    }

    #[inline]
    pub fn base(&self) -> usize {
        self.inner.base
    }

    #[inline]
    pub fn user_data(&self) -> &UserData {
        &self.inner.user_data
    }

    #[allow(unused_variables)]
    pub unsafe fn from_raw(
        name: CString,
        base: usize,
        dynamic: ElfDynamic,
        tls: Option<usize>,
        segments: ElfSegments,
        user_data: UserData,
    ) -> Self {
        Self {
            inner: Arc::new(RelocatedInner {
                name,
                base,
                symbols: SymbolData::new(&dynamic),
                dynamic: dynamic.dyn_ptr,
                #[cfg(feature = "tls")]
                tls: tls.map(|t| ElfTls {
                    id: t,
                    data: Box::new(()),
                }),
                unwind: None,
                segments,
                fini_fn: None,
                fini_array_fn: None,
                user_data: UserData::empty(),
                dep_libs: Vec::new(),
            }),
        }
    }

    /// Get a pointer to a function or static variable by symbol name.
    ///
    /// The symbol is interpreted as-is; no mangling is done. This means that symbols like `x::y` are
    /// most likely invalid.
    ///
    /// # Safety
    ///
    /// Users of this API must specify the correct type of the function or variable loaded.
    ///
    ///
    /// # Examples
    ///
    /// Given a loaded library:
    ///
    /// ```no_run
    /// # use ::dlopen_rs::ELFLibrary;
    /// let lib = ELFLibrary::from_file("/path/to/awesome.module")
    ///		.unwrap()
    ///		.relocate(&[])
    ///		.unwrap();
    /// ```
    ///
    /// Loading and using a function looks like this:
    ///
    /// ```no_run
    /// unsafe {
    ///     let awesome_function: Symbol<unsafe extern fn(f64) -> f64> =
    ///         lib.get("awesome_function").unwrap();
    ///     awesome_function(0.42);
    /// }
    /// ```
    ///
    /// A static variable may also be loaded and inspected:
    ///
    /// ```no_run
    /// unsafe {
    ///     let awesome_variable: Symbol<*mut f64> = lib.get("awesome_variable").unwrap();
    ///     **awesome_variable = 42.0;
    /// };
    /// ```
    pub unsafe fn get<'lib, T>(&'lib self, name: &str) -> Result<Symbol<'lib, T>> {
        self.inner
            .symbols
            .get_sym(&SymbolInfo::new(name))
            .map(|sym| Symbol {
                ptr: (self.base() + sym.st_value as usize) as _,
                pd: PhantomData,
            })
            .ok_or(find_symbol_error(format!("can not find symbol:{}", name)))
    }

    /// Attempts to load a versioned symbol from the dynamically-linked library.
    ///
    /// # Safety
    /// This function is unsafe because it involves raw pointer manipulation and
    /// dereferencing. The caller must ensure that the library handle is valid
    /// and that the symbol exists and has the correct type.
    ///
    /// # Parameters
    /// - `&'lib self`: A reference to the library instance from which the symbol will be loaded.
    /// - `name`: The name of the symbol to load.
    /// - `version`: The version of the symbol to load.
    ///
    /// # Returns
    /// If the symbol is found and has the correct type, this function returns
    /// `Ok(Symbol<'lib, T>)`, where `Symbol` is a wrapper around a raw function pointer.
    /// If the symbol cannot be found or an error occurs, it returns an `Err` with a message.
    ///
    /// # Examples
    /// ```
    /// let symbol = unsafe { lib.get_version::<fn()>>("function_name", "1.0").unwrap() };
    /// ```
    ///
    /// # Errors
    /// Returns a custom error if the symbol cannot be found, or if there is a problem
    /// retrieving the symbol.
    #[cfg(feature = "version")]
    pub unsafe fn get_version<'lib, T>(
        &'lib self,
        name: &str,
        version: &str,
    ) -> Result<Symbol<'lib, T>> {
        self.inner
            .symbols
            .get_sym(&SymbolInfo::new_with_version(name, version))
            .map(|sym| Symbol {
                ptr: (self.base() + sym.st_value as usize) as _,
                pd: PhantomData,
            })
            .ok_or(find_symbol_error(format!("can not find symbol:{}", name)))
    }
}

#[derive(Debug, Clone)]
pub struct Symbol<'lib, T: 'lib> {
    ptr: *mut (),
    pd: PhantomData<&'lib T>,
}

impl<'lib, T> ops::Deref for Symbol<'lib, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*(&self.ptr as *const *mut _ as *const T) }
    }
}

#[allow(unused)]
struct SymDef<'temp> {
    sym: &'temp ElfSymbol,
    base: usize,
    #[cfg(feature = "tls")]
    tls: Option<usize>,
}

impl<'temp> From<SymDef<'temp>> for *const () {
    fn from(symdef: SymDef<'temp>) -> Self {
        if symdef.sym.st_info & 0xf != STT_GNU_IFUNC {
            (symdef.base + symdef.sym.st_value as usize) as _
        } else {
            let ifunc: fn() -> usize =
                unsafe { core::mem::transmute(symdef.base + symdef.sym.st_value as usize) };
            ifunc() as _
        }
    }
}

pub trait StaticSymbol {
    fn symbol(name: &str) -> Option<*const ()>;
}

impl<T: ThreadLocal, U: Unwind> ElfDylib<T, U> {
    pub fn needed_libs(&self) -> &Vec<&str> {
        &self.needed_libs
    }

    pub fn relocate<S>(self, libs: impl AsRef<[RelocatedDylib]>) -> Self
    where
        S: StaticSymbol,
    {
        self.relocate_impl(libs.as_ref(), |name| S::symbol(name))
    }

    pub fn relocate_with<S, F>(self, libs: impl AsRef<[RelocatedDylib]>, func: F) -> Self
    where
        F: Fn(&str) -> Option<*const ()> + 'static,
        S: StaticSymbol,
    {
        let func = Box::new(func);
        let mut lib = self.relocate_impl(libs.as_ref(), |name| S::symbol(name).or(func(name)));
        lib.user_data.data.push(func);
        lib
    }

    fn relocate_impl<F>(mut self, libs: &[RelocatedDylib], find_symbol: F) -> Self
    where
        F: Fn(&str) -> Option<*const ()>,
    {
        let mut relocation = core::mem::take(&mut self.relocation);

        fn find_symdef<'a, T: ThreadLocal, U: Unwind>(
            elf_lib: &ElfDylib<T, U>,
            libs: &'a [RelocatedDylib],
            dynsym: &'a ElfSymbol,
            syminfo: SymbolInfo<'_>,
        ) -> Option<SymDef<'a>> {
            if dynsym.st_shndx != SHN_UNDEF {
                Some(SymDef {
                    sym: dynsym,
                    base: elf_lib.segments.base(),
                    #[cfg(feature = "tls")]
                    tls: elf_lib.tls.as_ref().map(|tls| unsafe { tls.module_id() }),
                })
            } else {
                let mut symbol = None;
                for lib in libs.iter() {
                    if let Some(sym) = lib.inner.symbols.get_sym(&syminfo) {
                        symbol = Some(SymDef {
                            sym,
                            base: lib.base(),
                            #[cfg(feature = "tls")]
                            tls: lib.inner.tls.as_ref().map(|tls| tls.id),
                        });
                        break;
                    }
                }
                symbol
            }
        }

        /*
            A Represents the addend used to compute the value of the relocatable field.
            B Represents the base address at which a shared object has been loaded into memory during execution.
            S Represents the value of the symbol whose index resides in the relocation entry.
        */

        if let Some(rela_array) = &mut relocation.pltrel {
            rela_array.relocate(|rela, idx, bitmap, deal_fail| {
                let r_type = rela.r_info as usize & REL_MASK;
                let r_sym = rela.r_info as usize >> REL_BIT;
                assert!(r_sym != 0);
                let (dynsym, syminfo) = self.symbols.rel_symbol(r_sym);
                let symbol = if let Some(symbol) = find_symbol(syminfo.name)
                    .or(find_symdef(&self, libs, dynsym, syminfo).map(|symdef| symdef.into()))
                {
                    symbol
                } else {
                    deal_fail(idx, bitmap);
                    return;
                };
                match r_type as _ {
                    // S
                    // 对于.rela.plt来说通常只有这一种重定位类型
                    REL_JUMP_SLOT => {
                        self.write_val(rela.r_offset as usize, symbol as usize);
                    }
                    _ => {
                        unreachable!()
                    }
                }
            });
        }

        if let Some(rela_array) = &mut relocation.dynrel {
            rela_array.relocate(|rela, idx, bitmap, deal_fail| {
                let r_type = rela.r_info as usize & REL_MASK;
                let r_sym = rela.r_info as usize >> REL_BIT;
                let mut name = "";
                let symdef = if r_sym != 0 {
                    let (dynsym, syminfo) = self.symbols.rel_symbol(r_sym);
                    name = syminfo.name;
                    find_symdef(&self, libs, dynsym, syminfo)
                } else {
                    None
                };

                match r_type as _ {
                    // REL_GOT: S  REL_SYMBOLIC: S + A
                    REL_GOT | REL_SYMBOLIC => {
                        let symbol = if let Some(symbol) =
                            find_symbol(name).or(symdef.map(|symdef| symdef.into()))
                        {
                            symbol
                        } else {
                            deal_fail(idx, bitmap);
                            return;
                        };
                        self.write_val(
                            rela.r_offset as usize,
                            symbol as usize + rela.r_addend as usize,
                        );
                    }
                    // B + A
                    REL_RELATIVE => {
                        self.write_val(
                            rela.r_offset as usize,
                            self.segments.base() + rela.r_addend as usize,
                        );
                    }
                    // ELFTLS
                    #[cfg(feature = "tls")]
                    REL_DTPMOD => {
                        if r_sym != 0 {
                            let symdef = if let Some(symdef) = symdef {
                                symdef
                            } else {
                                deal_fail(idx, bitmap);
                                return;
                            };
                            self.write_val(rela.r_offset as usize, symdef.tls.unwrap());
                        } else {
                            self.write_val(rela.r_offset as usize, unsafe {
                                self.tls.as_ref().unwrap().module_id()
                            });
                        }
                    }
                    #[cfg(feature = "tls")]
                    REL_DTPOFF => {
                        let symdef = if let Some(symdef) = symdef {
                            symdef
                        } else {
                            deal_fail(idx, bitmap);
                            return;
                        };
                        // offset in tls
                        let tls_val = (symdef.sym.st_value as usize + rela.r_addend as usize)
                            .wrapping_sub(TLS_DTV_OFFSET);
                        self.write_val(rela.r_offset as usize, tls_val);
                    }
                    _ => {
                        // REL_TPOFF：这种类型的重定位明显做不到，它是为静态模型设计的，这种方式
                        // 可以通过带偏移量的内存读取来获取TLS变量，无需使用__tls_get_addr，
                        // 实现它需要对要libc做修改，因为它要使用tp来访问thread local，
                        // 而线程栈里保存的东西完全是由libc控制的
                    }
                }
            });
        }

        self.relocation = relocation;
        self.dep_libs.extend_from_slice(libs);
        self
    }

    #[inline(always)]
    fn write_val(&self, offset: usize, val: usize) {
        unsafe {
            let rel_addr = (self.segments.base() + offset) as *mut usize;
            rel_addr.write(val)
        };
    }

    #[inline]
    pub fn is_finished(&self) -> bool {
        self.relocation.is_finished()
    }

    pub fn finish(mut self) -> Result<RelocatedDylib> {
        if !self.is_finished() {
            return Err(relocate_error(self.not_relocated()));
        }
        if let Some(init) = self.init_fn {
            init();
        }
        if let Some(init_array) = self.init_array_fn {
            for init in init_array {
                init();
            }
        }
        if let Some(relro) = self.relro {
            relro.relro()?;
        }

        Ok(RelocatedDylib {
            inner: Arc::new(RelocatedInner {
                name: self.name,
                base: self.segments.base(),
                symbols: self.symbols,
                dynamic: self.dynamic,
                #[cfg(feature = "tls")]
                tls: self.tls.map(|t| unsafe {
                    ElfTls {
                        id: t.module_id(),
                        data: Box::new(t),
                    }
                }),
                unwind: self.unwind.map(|val| Box::new(val) as Box<dyn Any>),
                segments: self.segments,
                fini_fn: self.fini_fn,
                fini_array_fn: self.fini_array_fn,
                user_data: self.user_data,
                dep_libs: self.dep_libs,
            }),
        })
    }

    #[cold]
    fn not_relocated(&mut self) -> String {
        let mut f = String::new();
        f.push_str(&format!(
            "{}: The symbols that have not been relocated:   ",
            self.name.to_str().unwrap()
        ));
        if let Some(array) = &mut self.relocation.pltrel {
            let mut iter = BitMapIterator::new(&mut array.state);
            while let Some((_, idx)) = iter.next() {
                let rela = &array.array[idx];
                let r_sym = rela.r_info as usize >> REL_BIT;
                if r_sym != 0 {
                    let (_, syminfo) = self.symbols.rel_symbol(r_sym);
                    f.push_str(&format!("[{}] ", syminfo.name));
                }
            }
        }
        if let Some(array) = &mut self.relocation.dynrel {
            let mut iter = BitMapIterator::new(&mut array.state);
            while let Some((_, idx)) = iter.next() {
                let rela = &array.array[idx];
                let r_sym = rela.r_info as usize >> REL_BIT;
                if r_sym != 0 {
                    let (_, syminfo) = self.symbols.rel_symbol(r_sym);
                    f.push_str(&format!("[{}] ", syminfo.name));
                }
            }
        }
        f
    }
}

#[derive(Default)]
pub(crate) struct ElfRelocation {
    pltrel: Option<ElfRelaArray>,
    dynrel: Option<ElfRelaArray>,
}

impl ElfRelocation {
    pub(crate) fn new(pltrel: Option<&'static [Rela]>, dynrel: Option<&'static [Rela]>) -> Self {
        let pltrel = pltrel.map(|array| ElfRelaArray {
            array,
            state: RelocateState {
                relocated: BitMap::new(array.len()),
                stage: RelocateStage::Init,
            },
        });
        let dynrel = dynrel.map(|array| ElfRelaArray {
            array,
            state: RelocateState {
                relocated: BitMap::new(array.len()),
                stage: RelocateStage::Init,
            },
        });
        Self { pltrel, dynrel }
    }

    fn is_finished(&self) -> bool {
        let mut finished = true;
        if let Some(array) = &self.pltrel {
            finished = array.is_finished();
        }
        if let Some(array) = &self.dynrel {
            finished = array.is_finished();
        }
        finished
    }
}

#[derive(PartialEq, Eq)]
enum RelocateStage {
    Init,
    Relocating(bool),
    Finish,
}

struct RelocateState {
    // 位图用于记录对应的项是否已经被重定位，已经重定位的项对应的bit会设为1
    relocated: BitMap,
    stage: RelocateStage,
}

struct ElfRelaArray {
    array: &'static [Rela],
    state: RelocateState,
}

struct BitMapIterator<'bitmap> {
    cur_bit: u32,
    index: usize,
    state: &'bitmap mut RelocateState,
}

impl<'bitmap> BitMapIterator<'bitmap> {
    fn new(state: &'bitmap mut RelocateState) -> Self {
        Self {
            cur_bit: state.relocated.unit(0),
            index: 0,
            state,
        }
    }

    fn next(&mut self) -> Option<(&mut RelocateState, usize)> {
        loop {
            let idx = self.cur_bit.trailing_ones();
            if idx == 32 {
                self.index += 1;
                if self.index == self.state.relocated.unit_count() {
                    break None;
                }
                self.cur_bit = self.state.relocated.unit(self.index);
            } else {
                self.cur_bit |= 1 << idx;
                break Some((self.state, self.index * 32 + idx as usize));
            }
        }
    }
}

impl ElfRelaArray {
    fn is_finished(&self) -> bool {
        let mut finished = true;
        if self.state.stage == RelocateStage::Init {
            return false;
        } else if self.state.stage != RelocateStage::Finish {
            for unit in &self.state.relocated.bitmap {
                if *unit != u32::MAX {
                    finished = false;
                    break;
                }
            }
        }
        finished
    }

    fn relocate(
        &mut self,
        f: impl Fn(&Rela, usize, &mut RelocateState, fn(usize, &mut RelocateState)),
    ) {
        match self.state.stage {
            RelocateStage::Init => {
                let deal_fail = |idx: usize, state: &mut RelocateState| {
                    state.relocated.clear(idx);
                    state.stage = RelocateStage::Relocating(false);
                };
                for (idx, rela) in self.array.iter().enumerate() {
                    f(rela, idx, &mut self.state, deal_fail);
                }
                if self.state.stage == RelocateStage::Init {
                    self.state.stage = RelocateStage::Finish;
                }
            }
            RelocateStage::Relocating(_) => {
                let deal_fail = |_idx: usize, state: &mut RelocateState| {
                    // 重定位失败，设置标识位
                    state.stage = RelocateStage::Relocating(false);
                };
                let mut iter = BitMapIterator::new(&mut self.state);
                while let Some((state, idx)) = iter.next() {
                    state.stage = RelocateStage::Relocating(true);
                    f(&self.array[idx], idx, state, deal_fail);
                    if state.stage == RelocateStage::Relocating(true) {
                        state.relocated.set(idx);
                    }
                }
            }
            RelocateStage::Finish => {}
        }
    }
}

pub(crate) struct BitMap {
    bitmap: Vec<u32>,
}

impl BitMap {
    fn new(size: usize) -> Self {
        let bitmap_size = (size + 31) / 32;
        let mut bitmap = Vec::with_capacity(bitmap_size);
        // 初始时全部标记为已重定位
        bitmap.resize(bitmap_size, u32::MAX);
        Self { bitmap }
    }

    fn unit(&self, index: usize) -> u32 {
        self.bitmap[index]
    }

    fn unit_count(&self) -> usize {
        self.bitmap.len()
    }

    fn set(&mut self, bit_index: usize) {
        let unit_index = bit_index / 32;
        let index = bit_index % 32;
        self.bitmap[unit_index] |= 1 << index;
    }

    fn clear(&mut self, bit_index: usize) {
        let unit_index = bit_index / 32;
        let index = bit_index % 32;
        self.bitmap[unit_index] &= !(1 << index);
    }
}
