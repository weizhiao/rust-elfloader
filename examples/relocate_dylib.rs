use elf_loader::load_dylib;
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
    let liba = load_dylib!("target/liba.so").unwrap();
    let libb = load_dylib!("target/libb.so").unwrap();
    let libc = load_dylib!("target/libc.so").unwrap();
    let a = liba.easy_relocate(&[], &pre_find).unwrap();
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
    let b = libb.easy_relocate([&a], &pre_find).unwrap();
    let f = unsafe { b.get::<fn() -> i32>("b").unwrap() };
    assert!(f() == 2);
    let c = libc.easy_relocate([&b], &pre_find).unwrap();
    let f = unsafe { c.get::<fn() -> i32>("c").unwrap() };
    assert!(f() == 3);
}
