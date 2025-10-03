use core::str;
use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
use std::collections::HashMap;

fn main() {
    unsafe { std::env::set_var("RUST_LOG", "trace") };
    env_logger::init();

    fn print(s: &str) {
        println!("{}", s);
    }

    let mut map = HashMap::new();
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    let mut loader = Loader::<MmapImpl>::new();
    let object = ElfFile::from_path("target/a.o").unwrap();
    let a = loader
        .load_relocatable(object, None)
        .unwrap()
        .relocate(&[], &pre_find)
        .unwrap();
    let b = loader
        .load_relocatable(ElfFile::from_path("target/b.o").unwrap(), None)
        .unwrap()
        .relocate(&[&a], &pre_find)
        .unwrap();
    let c = loader
        .load_relocatable(ElfFile::from_path("target/c.o").unwrap(), None)
        .unwrap()
        .relocate(&[&a, &b], &pre_find)
        .unwrap();
    let f = unsafe { a.get::<extern "C" fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
    let f = unsafe { b.get::<extern "C" fn() -> i32>("b").unwrap() };
    assert!(f() == 2);
    let f = unsafe { c.get::<extern "C" fn() -> i32>("c").unwrap() };
    assert!(f() == 3);
}
