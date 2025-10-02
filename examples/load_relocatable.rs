use core::str;
use std::{collections::HashMap, ffi::CStr};

use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};

fn main() {
    unsafe { std::env::set_var("RUST_LOG", "trace") };
    env_logger::init();
    // Load and relocate dynamic library liba.so
    fn print(s: *const i8) {
        let s = unsafe { CStr::from_ptr(s).to_str().unwrap() };
        println!("{}", s);
    }

    let mut map = HashMap::new();
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    let mut loader = Loader::<MmapImpl>::new();
    let object = ElfFile::from_path("a.o").unwrap();
    let a = loader.load_relocatable(object, None).unwrap();
    let a = a.relocate(&[], &pre_find).unwrap();
    let f = unsafe { a.get::<extern "C" fn()>("print_a").unwrap() };
    f();
}
