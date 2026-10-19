//! Linux/glibc end-to-end comparisons using unmodified real dependency graphs.
use criterion::{Criterion, criterion_group, criterion_main};
use elf_loader::{Linker, Loader, input::PathBuf};
use libloading::os::unix::{Library, RTLD_LOCAL, RTLD_NOW};
use std::hint::black_box;

#[path = "support/libraries.rs"]
mod libraries;
use libraries::{HostLibraries, LibraryCache, assert_native_unloaded, root_path};

const DEFAULT_WORKLOADS: &[&str] = &[
    "libz.so.1",
    "liblz4.so.1",
    "libzstd.so.1",
    "libsqlite3.so.0",
    "libpng16.so.16",
    "libfreetype.so.6",
];

fn bench_load_unload(c: &mut Criterion) {
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
    let cache = LibraryCache::new();
    let roots: Vec<_> = workloads
        .iter()
        .map(|&soname| (soname, root_path(&cache, soname)))
        .collect();
    // The fixed host allowlist is prepared outside timing for every selection.
    let hosts = HostLibraries::new(&cache);
    let mut group = c.benchmark_group("load_unload");
    for (soname, path) in roots {
        let linker = Linker::new()
            .with_loader(Loader::new())
            .with_resolver(cache.resolver());
        group.bench_function(format!("{soname}/relink"), |b| {
            let mut context = hosts.context();
            b.iter(|| {
                let loaded = linker
                    .run()
                    .load(
                        &mut context,
                        PathBuf::from(black_box(path.to_str().unwrap())),
                    )
                    .unwrap();
                // Includes graph detachment, finalizers and unmapping of all
                // ordinary dependencies, not just the root library.
                drop(loaded.release(&mut context).unwrap());
            });
            assert_eq!(context.load_order().count(), hosts.len());
        });
        // RTLD_NOLOAD checks only the native loader, not Relink mappings.
        assert_native_unloaded(&path);
        group.bench_function(format!("{soname}/libloading"), |b| {
            b.iter(|| {
                let library =
                    unsafe { Library::open(Some(black_box(&path)), RTLD_NOW | RTLD_LOCAL) }
                        .unwrap();
                drop(library);
            });
        });
        assert_native_unloaded(&path);
    }
    group.finish();
}

criterion_group!(benches, bench_load_unload);
criterion_main!(benches);
