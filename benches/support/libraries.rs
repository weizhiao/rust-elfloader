//! Root library lookup and the shared, explicit host-runtime boundary.
use elf::{ElfBytes, abi, endian::AnyEndian, symbol::Symbol};
use elf_loader::{AddressSpaceId, Domain};
use elf_loader::{
    LinkContext, Loader,
    arch::NativeArch,
    elf::ElfSymbolBind,
    image::{ModuleHandle, SyntheticModule, SyntheticSymbol},
    input::ElfFile,
    linker::{GraphModule, LdCache, SearchConfig, SearchPathResolver},
    relocation::RelocationArch,
    tls::{DefaultTlsResolver, TlsContext, TlsIndex, TlsInfo, TlsModuleId, TlsProvider},
};
use libloading::os::unix::{Library, RTLD_LOCAL, RTLD_NOW};
use std::{
    ffi::{CString, OsStr},
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
};

// A fixed host boundary, independent of the selected workloads. Candidate
// headers are checked against the native target, including the loader names.
const HOST_RUNTIMES: &[&str] = &[
    "libc.so.6",
    "libm.so.6",
    "libdl.so.2",
    "libpthread.so.0",
    "librt.so.1",
    "libgcc_s.so.1",
    "ld-linux-x86-64.so.2",
    "ld-linux.so.2",
    "ld-linux-aarch64.so.1",
];

/// Shared cache snapshot for root lookup and timed dependency lookup.
pub struct LibraryCache {
    cache: LdCache,
}

impl LibraryCache {
    pub fn new() -> Self {
        let cache = LdCache::load_system().expect("cannot read /etc/ld.so.cache");
        record_file("cache", "ld.so.cache", Path::new("/etc/ld.so.cache"));
        Self { cache }
    }

    /// Finds the first installed candidate accepted by the native loader backend.
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        let loader = Loader::new();
        self.cache.candidates(name).find_map(|path| {
            let file = ElfFile::from_path(path).ok()?;
            loader.read_ehdr(&file).ok()?;
            Some(PathBuf::from(OsStr::from_bytes(path.as_bytes())))
        })
    }

    pub fn resolver(&self) -> SearchPathResolver {
        SearchPathResolver::with_config(SearchConfig::new().cache(self.cache.clone()))
    }
}

/// Resolve and record the benchmark root before timing.
pub fn root_path(cache: &LibraryCache, name: &str) -> PathBuf {
    assert!(
        !HOST_RUNTIMES.contains(&name),
        "{name} is a preloaded host runtime, not a load/unload workload"
    );
    let path = cache
        .find(name)
        .unwrap_or_else(|| panic!("missing compatible {name}; see benches/README.md"));
    record_file("root", name, &path);
    path
}

pub fn assert_native_unloaded(path: &Path) {
    let name = CString::new(path.as_os_str().as_bytes()).unwrap();
    let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOLOAD | libc::RTLD_NOW) };
    if !handle.is_null() {
        unsafe { libc::dlclose(handle) };
        panic!(
            "{} remains loaded; refusing cached-handle measurements",
            path.display()
        );
    }
}

/// Machine-readable records consumed by ci/bench.sh to hash the exact files.
fn record_file(kind: &str, soname: &str, path: &Path) {
    eprintln!(
        "manifest\t{kind}\t{soname}\t{}",
        fs::canonicalize(path).unwrap().display()
    );
}

struct HostModuleData {
    _library: Arc<Library>,
}

struct HostLibrary {
    soname: String,
    module: ModuleHandle<NativeArch>,
}

impl HostLibrary {
    fn load(soname: &str, path: &Path) -> Self {
        let library = unsafe { Library::open(Some(path), RTLD_NOW | RTLD_LOCAL) }.unwrap();
        // libloading has no borrowed library-handle accessor. Restore ownership
        // immediately so the raw handle remains valid while building the bridge.
        let handle = library.into_raw();
        let library = Arc::new(unsafe { Library::from_raw(handle) });
        let bytes = fs::read(path).unwrap();
        let elf = ElfBytes::<AnyEndian>::minimal_parse(&bytes).unwrap();
        let module = unsafe { bridge_exports(soname, handle, &elf, library.clone()) };
        record_file("host", soname, path);
        Self {
            soname: soname.to_owned(),
            module: module.into(),
        }
    }
}

/// One shared set of native runtimes and their synthetic export tables.
pub struct HostLibraries {
    libraries: Vec<HostLibrary>,
}

impl HostLibraries {
    pub fn new(cache: &LibraryCache) -> Self {
        let libraries = HOST_RUNTIMES
            .iter()
            .filter_map(|soname| {
                cache
                    .find(soname)
                    .map(|path| HostLibrary::load(soname, &path))
            })
            .collect();
        Self { libraries }
    }

    pub fn context(&self) -> LinkContext<(), NativeArch, DefaultTlsResolver> {
        let mut context = LinkContext::new(
            Domain::native(AddressSpaceId::PROCESS)
                .with_tls(DefaultTlsResolver::new())
                .unwrap(),
        );
        let modules = self
            .libraries
            .iter()
            .map(|host| GraphModule::new(host.soname.clone(), host.module.clone()));
        context.insert_batch(modules).unwrap();
        context
    }

    pub fn len(&self) -> usize {
        self.libraries.len()
    }
}

fn is_bridgeable_export(symbol: &Symbol) -> bool {
    let visible = matches!(symbol.st_vis(), abi::STV_DEFAULT | abi::STV_PROTECTED);
    let exported = matches!(symbol.st_bind(), abi::STB_GLOBAL | abi::STB_WEAK);
    let supported_type = matches!(
        symbol.st_symtype(),
        abi::STT_FUNC | abi::STT_GNU_IFUNC | abi::STT_OBJECT | abi::STT_TLS
    );
    let version_marker = symbol.st_shndx == abi::SHN_ABS && symbol.st_value == 0;
    !symbol.is_undefined() && visible && exported && supported_type && !version_marker
}

type HostTlsFn = unsafe extern "C" fn(*const TlsIndex) -> *mut u8;

struct HostTls {
    handle: usize,
    mod_id: TlsModuleId,
    get_addr: HostTlsFn,
    _library: Arc<Library>,
}

// The retained library pins host TLS metadata; glibc supplies each thread's block.
unsafe impl TlsProvider for HostTls {
    unsafe fn resolve(
        &self,
        _context: &TlsContext,
        offset: usize,
    ) -> elf_loader::Result<NonNull<u8>> {
        let index = TlsIndex {
            ti_module: self.mod_id,
            ti_offset: offset.wrapping_sub(NativeArch::TLS_DTV_OFFSET),
        };
        NonNull::new(unsafe { (self.get_addr)(&index) }).ok_or_else(|| {
            elf_loader::error::TlsError::InvalidModuleId {
                mod_id: self.mod_id,
            }
            .into()
        })
    }

    unsafe fn get(&self, _context: &TlsContext) -> Option<NonNull<u8>> {
        let mut data: *mut u8 = std::ptr::null_mut();
        let status = unsafe {
            libc::dlinfo(
                self.handle as *mut libc::c_void,
                libc::RTLD_DI_TLS_DATA,
                (&mut data as *mut *mut u8).cast(),
            )
        };
        (status == 0).then(|| NonNull::new(data)).flatten()
    }
}

struct ExportVersion<'a> {
    name: &'a str,
    is_default: bool,
}

/// Export the entire supported host table, independent of any workload's imports.
/// The caller must keep the native handle alive while using the returned symbols.
unsafe fn bridge_exports(
    soname: &str,
    handle: *mut libc::c_void,
    elf: &ElfBytes<'_, AnyEndian>,
    library: Arc<Library>,
) -> SyntheticModule<NativeArch, HostModuleData> {
    let (symbols, strings) = elf.dynamic_symbol_table().unwrap().unwrap();
    let versions = elf.symbol_version_table().unwrap();
    let mut module = SyntheticModule::empty(
        &Domain::native(AddressSpaceId::PROCESS)
            .with_tls(DefaultTlsResolver::new())
            .unwrap(),
        soname.to_owned(),
    )
    .with_data(HostModuleData {
        _library: library.clone(),
    });
    if let Some(header) = elf
        .segments()
        .and_then(|headers| headers.iter().find(|header| header.p_type == abi::PT_TLS))
    {
        let mut id = 0usize;
        assert_eq!(
            unsafe {
                libc::dlinfo(
                    handle,
                    libc::RTLD_DI_TLS_MODID,
                    (&mut id as *mut usize).cast(),
                )
            },
            0
        );
        assert_ne!(id, 0);
        let provider = HostTls {
            handle: handle as usize,
            mod_id: TlsModuleId::new(id),
            get_addr: *unsafe { library.get::<HostTlsFn>(b"__tls_get_addr") }.unwrap(),
            _library: library,
        };
        let info = TlsInfo {
            vaddr: header.p_vaddr as usize,
            filesz: header.p_filesz as usize,
            memsz: header.p_memsz as usize,
            align: header.p_align as usize,
        };
        let tls =
            unsafe { DefaultTlsResolver::new().register_provider(info, None, provider) }.unwrap();
        module = module.with_tls(tls).unwrap();
    }
    for (index, symbol) in symbols.iter().enumerate().skip(1) {
        if !is_bridgeable_export(&symbol) {
            continue;
        }
        let name = strings.get_raw(symbol.st_name as usize).unwrap();
        let c_name = CString::new(name).unwrap();
        let version = versions
            .as_ref()
            .and_then(|table| table.get_definition(index).unwrap())
            .map(|mut definition| ExportVersion {
                name: definition.names.next().unwrap().unwrap(),
                is_default: !definition.hidden,
            });
        let mut export = if symbol.st_symtype() == abi::STT_TLS {
            SyntheticSymbol::tls(
                c_name.as_c_str(),
                symbol.st_value as usize,
                symbol.st_size as usize,
            )
        } else {
            let address = unsafe {
                match &version {
                    Some(version) => {
                        let c_version = CString::new(version.name).unwrap();
                        libc::dlvsym(handle, c_name.as_ptr(), c_version.as_ptr())
                    }
                    None => libc::dlsym(handle, c_name.as_ptr()),
                }
            };
            assert!(
                !address.is_null(),
                "cannot bridge {soname}:{}",
                c_name.to_string_lossy()
            );

            // Native lookup resolves IFUNCs; publish the resulting callable address.
            if symbol.st_symtype() == abi::STT_OBJECT {
                SyntheticSymbol::object(c_name.as_c_str(), address.cast(), symbol.st_size as usize)
            } else {
                SyntheticSymbol::function(c_name.as_c_str(), address.cast())
                    .with_size(symbol.st_size as usize)
            }
        };
        if symbol.st_bind() == abi::STB_WEAK {
            export = export.with_bind(ElfSymbolBind::WEAK);
        }
        if let Some(version) = version {
            export = export.with_version(version.name, version.is_default);
        }
        module.insert(export);
    }
    module
}
