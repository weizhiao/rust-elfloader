use elf_loader::{RelocatedDylib, load_dylib};
use std::{fs::File, io::Read};

fn main() {
    unsafe { std::env::set_var("RUST_LOG", "trace") };
    env_logger::init();
    let mut file = File::open("target/liba.so").unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    let liba = load_dylib!("target/liba.so", &bytes).unwrap();
    let empty: [RelocatedDylib; 0] = [];
    let a = liba.easy_relocate(&empty, &|_| None).unwrap();
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    println!("{}", f());
}
