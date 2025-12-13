mod common;

use common::get_path;
use elf_loader::{Elf, Relocatable, load, load_dylib, load_exec};
use rstest::rstest;
use std::{fs::File, io::Read};

#[rstest]
fn load_from_memory() {
    let path = get_path("liba.so");
    let mut file = File::open(&path).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    let liba = load_dylib!(path.to_str().unwrap(), &bytes).unwrap();
    let a = liba.relocator().relocate().unwrap();
    let f = unsafe { *a.get::<fn() -> i32>("a").unwrap() };
    assert_eq!(f(), 1);
}

#[rstest]
fn wrong_name_fails() {
    let _ = load_dylib!("target/this_location_is_definitely_non existent:^~")
        .err()
        .unwrap();
}

#[rstest]
fn type_mismatch() {
    let _ = load_exec!(get_path("liba.so").to_str().unwrap())
        .err()
        .unwrap();
}

#[rstest]
fn load_elf() {
    let liba = load!(get_path("liba.so").to_str().unwrap()).unwrap();
    assert!(matches!(liba, Elf::Dylib(_)));
    let a = liba.relocator().relocate().unwrap().into_dylib().unwrap();
    let f = unsafe { *a.get::<fn() -> i32>("a").unwrap() };
    assert_eq!(f(), 1);
}
