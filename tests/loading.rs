mod common;

use common::get_path;
use elf_loader::{Elf, Relocatable, load, load_dylib, load_exec};
use rstest::rstest;
use std::{fs::File, io::Read};

#[rstest]
fn load_from_memory() {
    // Try to find a dynamic fixture produced by build.rs
    let names = [
        "libx86_64_dynamic.so",
        "libaarch64_dynamic.so",
        "libriscv64_dynamic.so",
    ];
    let mut path = None;
    for n in &names {
        let p = get_path(n);
        if p.exists() {
            path = Some(p);
            break;
        }
    }
    let path = match path {
        Some(p) => p,
        None => {
            eprintln!("Skipping test: no dynamic fixture found");
            return;
        }
    };

    let mut file = File::open(&path).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    let lib = load_dylib!(path.to_str().unwrap(), &bytes).unwrap();
    let _ = lib.relocator().relocate().unwrap();
}

#[rstest]
fn wrong_name_fails() {
    let _ = load_dylib!("target/this_location_is_definitely_non existent:^~")
        .err()
        .unwrap();
}

#[rstest]
fn type_mismatch() {
    // Use any dynamic fixture for type mismatch test
    let names = [
        "libx86_64_dynamic.so",
        "libaarch64_dynamic.so",
        "libriscv64_dynamic.so",
    ];
    let mut path = None;
    for n in &names {
        let p = get_path(n);
        if p.exists() {
            path = Some(p);
            break;
        }
    }
    let path = match path {
        Some(p) => p,
        None => {
            eprintln!("Skipping test: no dynamic fixture found");
            return;
        }
    };

    let _ = load_exec!(path.to_str().unwrap()).err().unwrap();
}

#[rstest]
fn load_elf() {
    let names = [
        "libx86_64_dynamic.so",
        "libaarch64_dynamic.so",
        "libriscv64_dynamic.so",
    ];
    let mut path = None;
    for n in &names {
        let p = get_path(n);
        if p.exists() {
            path = Some(p);
            break;
        }
    }
    let path = match path {
        Some(p) => p,
        None => {
            eprintln!("Skipping test: no dynamic fixture found");
            return;
        }
    };

    let lib = load!(path.to_str().unwrap()).unwrap();
    assert!(matches!(lib, Elf::Dylib(_)));
    let _ = lib.relocator().relocate().unwrap().into_dylib().unwrap();
}
