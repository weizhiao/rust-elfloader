use std::{collections::HashMap, ffi::CStr};
use windows_elf_loader::WinElfLoader;

fn main() {
    extern "sysv64" fn print(s: *const i8) {
        let s = unsafe { CStr::from_ptr(s).to_str().unwrap() };
        println!("{}", s);
    }

    let mut map = HashMap::new();
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    let mut loader = WinElfLoader::new();
    let liba = loader
        .load_dylib(
            "liba",
            include_bytes!("../../target/x86_64-unknown-linux-gnu/release/liba.so"),
        )
        .unwrap();
    let libb = loader
        .load_dylib(
            "libb",
            include_bytes!("../../target/x86_64-unknown-linux-gnu/release/libb.so"),
        )
        .unwrap();
    let libc = loader
        .load_dylib(
            "libc",
            include_bytes!("../../target/x86_64-unknown-linux-gnu/release/libc.so"),
        )
        .unwrap();
    let a = liba.easy_relocate([], &pre_find).unwrap();
    let f = unsafe { a.get::<extern "sysv64" fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
    let b = libb.easy_relocate([&a], &pre_find).unwrap();
    let f = unsafe { b.get::<extern "sysv64" fn() -> i32>("b").unwrap() };
    assert!(f() == 2);
    let c = libc.easy_relocate([&b], &pre_find).unwrap();
    let f = unsafe { c.get::<extern "sysv64" fn() -> i32>("c").unwrap() };
    assert!(f() == 3);
}
