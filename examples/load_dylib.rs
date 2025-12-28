use elf_loader::Loader;
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
    let liba = Loader::new()
        .load_dylib("target/liba.so")
        .unwrap()
        .relocator()
        .pre_find(&pre_find)
        .relocate()
        .unwrap();
    // Call function a in liba.so
    let f = unsafe { liba.get::<fn() -> i32>("a").unwrap() };
    println!("{}", f());
}
