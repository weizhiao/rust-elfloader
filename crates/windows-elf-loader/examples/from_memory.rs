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
        .load_dylib("liba", include_bytes!("../example_dylib/liba.so"))
        .unwrap();
    let libb = loader
        .load_dylib("libb", include_bytes!("../example_dylib/libb.so"))
        .unwrap();
    let libc = loader
        .load_dylib("libc", include_bytes!("../example_dylib/libc.so"))
        .unwrap();
    let a = liba.relocate([], &pre_find).unwrap();
    let f = unsafe { a.get::<extern "sysv64" fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
    let b = libb.relocate([&a], &pre_find).unwrap();
    let f = unsafe { b.get::<extern "sysv64" fn() -> i32>("b").unwrap() };
    assert!(f() == 2);
    let c = libc.relocate([&b], &pre_find).unwrap();
    let f = unsafe { c.get::<extern "sysv64" fn() -> i32>("c").unwrap() };
    assert!(f() == 3);
}
