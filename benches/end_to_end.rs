//! Linux/glibc end-to-end comparisons using unmodified real dependency graphs.
use criterion::{Criterion, criterion_group, criterion_main};
use elf_loader::{Linker, input::PathBuf};
use libloading::os::unix::{Library, RTLD_LOCAL, RTLD_NOW};
use std::hint::black_box;

#[path = "support/real_libraries.rs"]
mod real_libraries;
#[path = "support/real_validate.rs"]
mod real_validate;
use real_libraries::{HostLibraries, LibraryCatalog, LibraryFixture, native_files};
use real_validate::Workload;

fn bench_real(c: &mut Criterion) {
    assert!(
        cfg!(all(target_os = "linux", target_env = "gnu")),
        "this suite requires Linux/glibc"
    );
    let catalog = LibraryCatalog::new();
    let fixtures: Vec<_> = Workload::ALL
        .iter()
        .map(|workload| (*workload, LibraryFixture::new(&catalog, workload.soname())))
        .collect();
    // One full host export table shared by every workload, prepared outside timing.
    let hosts = HostLibraries::new(
        &catalog,
        fixtures
            .iter()
            .flat_map(|(_, fixture)| fixture.host_names.iter().cloned())
            .collect(),
    );
    // Full system-cache lookup replaces fixture-derived directories. Keep the
    // baseline separate from the earlier end_to_end_graph measurements.
    let mut group = c.benchmark_group("end_to_end_cache");
    for (workload, fixture) in fixtures {
        let soname = workload.soname();
        let linker = Linker::new().resolver(catalog.resolver());
        fixture.assert_absent();
        // Validate native and Relink separately: native copies cannot accidentally
        // satisfy the Relink graph's ordinary-library dependencies.
        let before = native_files();
        let expected = {
            let native =
                unsafe { Library::open(Some(&fixture.root_path), RTLD_NOW | RTLD_LOCAL) }.unwrap();
            fixture.assert_native_files(&before);
            let result = unsafe { workload.validate(|name| *native.get::<*mut ()>(name).unwrap()) };
            fixture.assert_native_files(&before);
            result
        };
        fixture.assert_absent();
        assert_eq!(
            native_files(),
            before,
            "native validation retained unexpected libraries"
        );
        {
            let mut context = hosts.context();
            let loaded = linker
                .run()
                .load(
                    &mut context,
                    PathBuf::from(fixture.root_path.to_str().unwrap()),
                )
                .unwrap();
            fixture.assert_relink_files(&context, hosts.len());
            let result = unsafe {
                workload.validate(|name| {
                    *context
                        .module(loaded.root())
                        .unwrap()
                        .get::<*mut ()>(name)
                        .unwrap()
                })
            };
            assert_eq!(result, expected);
            let unloaded = loaded.release(&mut context).unwrap();
            assert_eq!(unloaded.len(), fixture.real_libraries.len());
            drop(unloaded);
            assert_eq!(context.load_order().count(), hosts.len());
        }
        fixture.assert_absent();
        assert_eq!(
            native_files(),
            before,
            "Relink validation loaded unexpected native libraries"
        );
        eprintln!(
            "validated {soname}: {expected}; real_modules={}",
            fixture.real_libraries.len()
        );
        group.bench_function(format!("{soname}/load_unload/relink"), |b| {
            let mut context = hosts.context();
            b.iter(|| {
                let loaded = linker
                    .run()
                    .load(
                        &mut context,
                        PathBuf::from(black_box(fixture.root_path.to_str().unwrap())),
                    )
                    .unwrap();
                // Includes graph detachment, finalizers and unmapping of all
                // ordinary dependencies, not just the root library.
                drop(loaded.release(&mut context).unwrap());
            });
            assert_eq!(context.load_order().count(), hosts.len());
        });
        fixture.assert_absent();
        group.bench_function(format!("{soname}/load_unload/libloading"), |b| {
            b.iter(|| {
                let library = unsafe {
                    Library::open(Some(black_box(&fixture.root_path)), RTLD_NOW | RTLD_LOCAL)
                }
                .unwrap();
                drop(library);
            });
        });
        fixture.assert_absent();
    }
    group.finish();
}

criterion_group!(benches, bench_real);
criterion_main!(benches);
