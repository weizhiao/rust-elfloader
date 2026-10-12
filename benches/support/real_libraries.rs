//! Real dependency discovery and the shared, explicit host-runtime boundary.
use elf::{ElfBytes, abi, endian::AnyEndian, symbol::Symbol};
use elf_loader::{
    LinkContext, Loader,
    elf::ElfSymbolBind,
    image::{ModuleHandle, SyntheticModule, SyntheticSymbol},
    input::ElfFile,
    linker::{GraphModule, LdCache, SearchConfig, SearchPathResolver},
    runtime::DomainId,
};
use libloading::os::unix::{Library, RTLD_LOCAL, RTLD_NOW};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString, OsStr},
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf as StdPathBuf},
};

// No fallback: every other DT_NEEDED must be loaded from a real ELF file.
pub fn is_host_runtime(name: &str) -> bool {
    matches!(
        name,
        "libc.so.6"
            | "libm.so.6"
            | "libdl.so.2"
            | "libpthread.so.0"
            | "librt.so.1"
            | "libgcc_s.so.1"
            | "ld-linux-x86-64.so.2"
            | "ld-linux.so.2"
            | "ld-linux-aarch64.so.1"
    )
}

/// Shared cache snapshot for fixture discovery and timed dependency lookup.
pub struct LibraryCatalog {
    cache: LdCache,
}

impl LibraryCatalog {
    pub fn new() -> Self {
        let cache = LdCache::load_system().expect("cannot read /etc/ld.so.cache");
        record_library("cache", "ld.so.cache", Path::new("/etc/ld.so.cache"));
        Self { cache }
    }

    /// Finds the first installed candidate accepted by the native loader backend.
    pub fn locate(&self, name: &str) -> StdPathBuf {
        let loader = Loader::new();
        self.cache
            .candidates(name)
            .find_map(|path| {
                let file = ElfFile::from_path(path).ok()?;
                loader.read_ehdr(&file).ok()?;
                Some(StdPathBuf::from(OsStr::from_bytes(path.as_bytes())))
            })
            .unwrap_or_else(|| panic!("missing compatible {name}; see benches/README.md"))
    }

    pub fn resolver(&self) -> SearchPathResolver {
        SearchPathResolver::with_config(SearchConfig::new().cache(self.cache.clone()))
    }
}

/// The root and its ordinary dependency closure, discovered before timing.
pub struct LibraryFixture {
    pub root_path: StdPathBuf,
    pub real_libraries: BTreeMap<String, StdPathBuf>,
    pub host_names: BTreeSet<String>,
}

impl LibraryFixture {
    pub fn new(catalog: &LibraryCatalog, root: &str) -> Self {
        let mut fixture = Self {
            root_path: catalog.locate(root),
            real_libraries: BTreeMap::new(),
            host_names: BTreeSet::new(),
        };
        let mut pending = vec![root.to_owned()];
        while let Some(soname) = pending.pop() {
            if is_host_runtime(&soname) {
                fixture.host_names.insert(soname);
                continue;
            }
            if fixture.real_libraries.contains_key(&soname) {
                continue;
            }
            let path = catalog.locate(&soname);
            let dependencies = inspect_real_library(&soname, &path);
            pending.extend(dependencies);
            fixture.real_libraries.insert(soname, path);
        }
        fixture
    }

    pub fn assert_absent(&self) {
        for path in self.real_libraries.values() {
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
    }

    pub fn assert_native_files(&self, before: &BTreeSet<StdPathBuf>) {
        let after = native_files();
        assert_eq!(
            after.difference(before).cloned().collect::<BTreeSet<_>>(),
            self.canonical_files(),
            "native loader used a different dependency graph or different binaries"
        );
    }

    pub fn assert_relink_files(&self, context: &LinkContext<()>, host_count: usize) {
        let mut files = BTreeSet::new();
        for id in context.load_order() {
            let module = context.module(id).unwrap();
            // Synthetic host modules have no loaded file path.
            if let Some(search) = module.search() {
                let path = OsStr::from_bytes(search.path().as_bytes());
                files.insert(fs::canonicalize(path).unwrap());
            }
        }
        assert_eq!(
            files,
            self.canonical_files(),
            "Relink used different dependency binaries"
        );
        assert_eq!(
            context.load_order().count(),
            host_count + self.real_libraries.len(),
            "dependency duplicated or missing"
        );
    }

    fn canonical_files(&self) -> BTreeSet<StdPathBuf> {
        self.real_libraries
            .values()
            .map(|path| fs::canonicalize(path).unwrap())
            .collect()
    }
}

/// Inspect metadata only; loading and relocation remain inside the benchmark.
fn inspect_real_library(soname: &str, path: &Path) -> Vec<String> {
    let bytes = fs::read(path).unwrap();
    let elf = ElfBytes::<AnyEndian>::minimal_parse(&bytes).unwrap();
    let (symbols, _) = elf.dynamic_symbol_table().unwrap().unwrap();
    for symbol in symbols.iter().filter(|symbol| symbol.is_undefined()) {
        assert_ne!(
            symbol.st_symtype(),
            abi::STT_TLS,
            "host TLS imports require an explicit adapter: {soname}"
        );
    }
    let dependencies = needed_libraries(&elf);
    eprintln!("dependency {soname}: {}", dependencies.join(", "));
    record_library("real", soname, path);
    dependencies
}

fn needed_libraries(elf: &ElfBytes<'_, AnyEndian>) -> Vec<String> {
    let (_, strings) = elf.dynamic_symbol_table().unwrap().unwrap();
    elf.dynamic()
        .unwrap()
        .unwrap()
        .iter()
        .filter(|entry| entry.d_tag == abi::DT_NEEDED)
        .map(|entry| strings.get(entry.d_val() as usize).unwrap().to_owned())
        .collect()
}

/// Machine-readable records consumed by ci/bench.sh to hash the exact files.
fn record_library(kind: &str, soname: &str, path: &Path) {
    eprintln!(
        "manifest\t{kind}\t{soname}\t{}",
        fs::canonicalize(path).unwrap().display()
    );
}

/// Snapshot the native loader's file set for graph and lifetime checks.
pub fn native_files() -> BTreeSet<StdPathBuf> {
    unsafe extern "C" fn collect_loaded_file(
        info: *mut libc::dl_phdr_info,
        _: usize,
        data: *mut libc::c_void,
    ) -> libc::c_int {
        // glibc supplies a live entry; data points to the set below for this call.
        let name = unsafe { CStr::from_ptr((*info).dlpi_name) }.to_bytes();
        if let Ok(path) = fs::canonicalize(OsStr::from_bytes(name)) {
            unsafe { &mut *data.cast::<BTreeSet<StdPathBuf>>() }.insert(path);
        }
        0
    }
    let mut files = BTreeSet::new();
    unsafe {
        libc::dl_iterate_phdr(
            Some(collect_loaded_file),
            (&mut files as *mut BTreeSet<StdPathBuf>).cast(),
        )
    };
    files
}

struct HostLibrary {
    soname: String,
    module: ModuleHandle,
    // Synthetic symbols borrow native addresses; keep their library loaded.
    _native_library: Library,
}

impl HostLibrary {
    fn load(catalog: &LibraryCatalog, soname: String) -> (Self, Vec<String>) {
        assert!(
            is_host_runtime(&soname),
            "host runtime depends on non-host {soname}"
        );
        let path = catalog.locate(&soname);
        let library = unsafe { Library::open(Some(&path), RTLD_NOW | RTLD_LOCAL) }.unwrap();
        // libloading has no borrowed library-handle accessor. Restore ownership
        // immediately so the raw handle remains valid while building the bridge.
        let handle = library.into_raw();
        let library = unsafe { Library::from_raw(handle) };
        let bytes = fs::read(&path).unwrap();
        let elf = ElfBytes::<AnyEndian>::minimal_parse(&bytes).unwrap();
        let dependencies = needed_libraries(&elf);
        let module = unsafe { bridge_host_exports(&soname, handle, &elf) };
        eprintln!("host {soname}: {} exports", module.len());
        record_library("host", &soname, &path);
        (
            Self {
                soname,
                module: module.into(),
                _native_library: library,
            },
            dependencies,
        )
    }
}

/// One shared set of native runtimes and their synthetic export tables.
pub struct HostLibraries {
    libraries: Vec<HostLibrary>,
}

impl HostLibraries {
    pub fn new(catalog: &LibraryCatalog, names: BTreeSet<String>) -> Self {
        let mut libraries = Vec::new();
        let mut pending: Vec<_> = names.into_iter().collect();
        let mut visited = BTreeSet::new();
        while let Some(soname) = pending.pop() {
            if !visited.insert(soname.clone()) {
                continue;
            }
            let (library, dependencies) = HostLibrary::load(catalog, soname);
            pending.extend(dependencies);
            libraries.push(library);
        }
        Self { libraries }
    }

    pub fn context(&self) -> LinkContext<()> {
        let mut context = LinkContext::new(DomainId::PROCESS);
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
    // TLS addresses are thread-relative and cannot use this absolute-address bridge.
    let supported_type = matches!(
        symbol.st_symtype(),
        abi::STT_FUNC | abi::STT_GNU_IFUNC | abi::STT_OBJECT
    );
    let version_marker = symbol.st_shndx == abi::SHN_ABS && symbol.st_value == 0;
    !symbol.is_undefined() && visible && exported && supported_type && !version_marker
}

struct ExportVersion<'a> {
    name: &'a str,
    is_default: bool,
}

/// Export the entire supported host table, independent of any fixture's imports.
/// The caller must keep the native handle alive while using the returned symbols.
unsafe fn bridge_host_exports(
    soname: &str,
    handle: *mut libc::c_void,
    elf: &ElfBytes<'_, AnyEndian>,
) -> SyntheticModule {
    let (symbols, strings) = elf.dynamic_symbol_table().unwrap().unwrap();
    let versions = elf.symbol_version_table().unwrap();
    let mut module = SyntheticModule::empty(soname.to_owned());
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
        let mut export = if symbol.st_symtype() == abi::STT_OBJECT {
            SyntheticSymbol::object(name, address.cast(), symbol.st_size as usize)
        } else {
            SyntheticSymbol::function(name, address.cast()).with_size(symbol.st_size as usize)
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
