use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
use std::collections::HashMap;

fn main() {
    fn print(s: &str) {
        println!("{}", s);
    }

    // Symbols required by dynamic library liba.so
    let mut map = HashMap::new();
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    // Load dynamic library liba.so
    let mut loader = Loader::<MmapImpl>::new();
    let liba = loader
        .easy_load_dylib(ElfFile::from_path("target/liba.so").unwrap())
        .unwrap();
    // Relocate symbols in liba.so
    let a = liba.easy_relocate([].iter(), &pre_find).unwrap();
    // Call function a in liba.so
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    println!("{}", f());
}
