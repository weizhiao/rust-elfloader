//! Root library lookup and the shared, explicit host-runtime boundary.
use elf_loader::{
    AddressSpaceId, Domain, LinkContext, Loader, host,
    image::ModuleHandle,
    input::{ElfFile, Path as ElfPath},
    linker::{GraphModule, LdCache, SearchConfig, SearchPathResolver},
    tls::DefaultTlsResolver,
};
use std::{
    ffi::{CString, OsStr},
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
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

struct HostModule {
    soname: String,
    module: ModuleHandle,
}

impl HostModule {
    fn load(
        domain: &Domain<elf_loader::arch::NativeArch, DefaultTlsResolver>,
        soname: &str,
        path: &Path,
    ) -> Self {
        let module =
            unsafe { host::open(domain, ElfPath::new(path.as_os_str().as_bytes()), soname) }
                .unwrap();
        record_file("host", soname, path);
        Self {
            soname: soname.to_owned(),
            module,
        }
    }
}

/// One shared set of native runtimes and their synthetic export tables.
pub struct HostLibraries {
    domain: Domain<elf_loader::arch::NativeArch, DefaultTlsResolver>,
    libraries: Vec<HostModule>,
}

impl HostLibraries {
    pub fn new(cache: &LibraryCache) -> Self {
        let domain = Domain::native(AddressSpaceId::PROCESS)
            .with_tls(DefaultTlsResolver::new())
            .unwrap();
        let libraries = HOST_RUNTIMES
            .iter()
            .filter_map(|soname| {
                cache
                    .find(soname)
                    .map(|path| HostModule::load(&domain, soname, &path))
            })
            .collect();
        Self { domain, libraries }
    }

    pub fn context(&self) -> LinkContext<(), elf_loader::arch::NativeArch, DefaultTlsResolver> {
        let mut context = LinkContext::new(self.domain.clone());
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
