use criterion::{Criterion, criterion_group, criterion_main};
use elf_loader::{Loader, Relocatable, object::ElfFile};
use libloading::Library;

fn load_benchmark(c: &mut Criterion) {
    let path = "target/liba.so";
    c.bench_function("elf_loader:new", |b| {
        b.iter(|| {
            let mut loader = Loader::new();
            let liba = loader
                .load_dylib(ElfFile::from_path(&path).unwrap())
                .unwrap();
            let _ = liba.relocator().relocate().unwrap();
        });
    });
    c.bench_function("libloading:new", |b| {
        b.iter(|| {
            unsafe { Library::new(&path).unwrap() };
        })
    });
}

fn get_symbol_benchmark(c: &mut Criterion) {
    let path = "target/liba.so";
    let mut loader = Loader::new();
    let liba = loader
        .load_dylib(ElfFile::from_path(&path).unwrap())
        .unwrap();
    let lib1 = liba.relocator().relocate().unwrap();
    let lib2 = unsafe { Library::new(path).unwrap() };
    c.bench_function("elf_loader:get", |b| {
        b.iter(|| unsafe { lib1.get::<fn(i32, i32) -> i32>("a").unwrap() })
    });
    c.bench_function("libloading:get", |b| {
        b.iter(|| {
            unsafe { lib2.get::<fn(i32, i32) -> i32>("a".as_bytes()).unwrap() };
        })
    });
}

criterion_group!(benches, load_benchmark, get_symbol_benchmark);
criterion_main!(benches);
