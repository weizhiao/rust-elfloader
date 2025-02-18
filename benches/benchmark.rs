use std::{collections::HashMap, path::Path, ptr::null};

use criterion::{Criterion, criterion_group, criterion_main};
use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
use libloading::Library;

fn load_benchmark(c: &mut Criterion) {
    let path = Path::new("target/liba.so");
    let mut map = HashMap::new();
    map.insert("__gmon_start__", null());
    map.insert("__cxa_finalize", null());
    map.insert("_ITM_registerTMCloneTable", null());
    map.insert("_ITM_deregisterTMCloneTable", null());
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    c.bench_function("elf_loader:new", |b| {
        b.iter(|| {
            let loader = Loader::<MmapImpl>::new();
            let liba = loader
                .easy_load_dylib(ElfFile::from_path("target/liba.so").unwrap())
                .unwrap();
            let _ = liba.easy_relocate([].iter(), &pre_find).unwrap();
        });
    });
    c.bench_function("libloading:new", |b| {
        b.iter(|| {
            unsafe { Library::new(path).unwrap() };
        })
    });
}

fn get_symbol_benchmark(c: &mut Criterion) {
    let path = Path::new("target/liba.so");
    let mut map = HashMap::new();
    map.insert("__gmon_start__", null());
    map.insert("__cxa_finalize", null());
    map.insert("_ITM_registerTMCloneTable", null());
    map.insert("_ITM_deregisterTMCloneTable", null());
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    let loader = Loader::<MmapImpl>::new();
    let liba = loader
        .easy_load_dylib(ElfFile::from_path("target/liba.so").unwrap())
        .unwrap();
    let lib1 = liba.easy_relocate([].iter(), &pre_find).unwrap();
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
