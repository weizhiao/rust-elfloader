//! Linux/glibc load and teardown comparisons using unmodified real dependency graphs.
use criterion::{Criterion, criterion_group, criterion_main};
use elf_loader::{Linker, Loader, input::PathBuf};
use libloading::os::unix::{Library, RTLD_LOCAL, RTLD_NOW};
use std::{ffi::CString, hint::black_box, os::unix::ffi::OsStrExt, path::Path};

#[path = "support/libraries.rs"]
mod libraries;
#[path = "support/timing.rs"]
mod timing;
use libraries::{HostLibraries, LibraryCache, root_path};

// Only sample load and teardown; total would repeat the same work a third time.
const PHASES: [&str; 2] = ["load", "teardown"];

const DEFAULT_WORKLOADS: &[&str] = &[
    "libz.so.1",
    "liblz4.so.1",
    "libzstd.so.1",
    "libsqlite3.so.0",
    "libpng16.so.16",
    "libfreetype.so.6",
];

fn bench_load_teardown(c: &mut Criterion) {
    assert!(
        cfg!(all(target_os = "linux", target_env = "gnu")),
        "this suite requires Linux/glibc"
    );
    let selected = std::env::var("RELINK_BENCH_WORKLOAD").unwrap_or_default();
    let workloads = if selected.is_empty() {
        DEFAULT_WORKLOADS.to_vec()
    } else {
        vec![selected.as_str()]
    };
    for soname in &workloads {
        assert!(
            !soname.contains(['/', ':']) && !soname.chars().any(char::is_control),
            "expected a library SONAME, got {soname:?}"
        );
        eprintln!("workload\t{soname}");
    }
    let cache = LibraryCache::new(true);
    let roots: Vec<_> = workloads
        .iter()
        .map(|&soname| (soname, root_path(&cache, soname)))
        .collect();
    // The fixed host allowlist is prepared outside timing for every selection.
    let hosts = HostLibraries::new(&cache);
    // A new group keeps results separate from the old reused-context measurements.
    let mut group = c.benchmark_group("load_teardown");
    for (soname, path) in roots {
        let linker = Linker::new()
            .with_loader(Loader::new())
            .with_resolver(cache.resolver());
        for (index, phase) in PHASES.iter().enumerate() {
            group.bench_function(format!("{soname}/relink/{phase}"), |b| {
                b.iter_custom(|iterations| {
                    timing::measure(
                        iterations,
                        || hosts.context(),
                        |context| {
                            linker
                                .run()
                                .load(context, PathBuf::from(black_box(path.to_str().unwrap())))
                                .unwrap()
                        },
                        |context, loaded| {
                            drop(loaded.release(context).unwrap());
                        },
                    )
                    .phases()[index]
                });
            });
        }
        // dlclose cannot end a glibc namespace containing a NODELETE root.
        // Detect this before collecting misleading cached-handle measurements.
        if native_is_loaded(&path) {
            eprintln!("skip\t{soname}\tlibloading\troot already resident before measurement");
            continue;
        }
        drop(unsafe { Library::open(Some(&path), RTLD_NOW | RTLD_LOCAL) }.unwrap());
        if native_is_loaded(&path) {
            eprintln!(
                "skip\t{soname}\tlibloading\troot retained after dlclose (NODELETE or GNU-unique bindings)"
            );
            continue;
        }
        for (index, phase) in PHASES.iter().enumerate() {
            group.bench_function(format!("{soname}/libloading/{phase}"), |b| {
                b.iter_custom(|iterations| {
                    timing::measure(
                        iterations,
                        || (),
                        |_| unsafe {
                            Library::open(Some(black_box(&path)), RTLD_NOW | RTLD_LOCAL).unwrap()
                        },
                        |_, library| drop(library),
                    )
                    .phases()[index]
                });
            });
            assert!(
                !native_is_loaded(&path),
                "{} became resident during measurement",
                path.display()
            );
        }
    }
    group.finish();
}

fn native_is_loaded(path: &Path) -> bool {
    let name = CString::new(path.as_os_str().as_bytes()).unwrap();
    let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOLOAD | libc::RTLD_NOW) };
    if !handle.is_null() {
        unsafe { libc::dlclose(handle) };
    }
    !handle.is_null()
}

criterion_group!(benches, bench_load_teardown);
criterion_main!(benches);
