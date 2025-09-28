use elf_loader::{RelocatedDylib, load_dylib};
use std::collections::HashMap;

fn main() {
    fn print(s: &str) {
        println!("{}", s);
    }

    // Symbols required by dynamic library liba.so
    let mut map = HashMap::new();
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    // Load and relocate dynamic library liba.so
    let empty: [RelocatedDylib; 0] = [];
    let liba = load_dylib!("target/liba.so")
        .unwrap()
        .easy_relocate(&empty, &pre_find)
        .unwrap();
    // Call function a in liba.so
    let f = unsafe { liba.get::<fn() -> i32>("a").unwrap() };
    println!("{}", f());
}
