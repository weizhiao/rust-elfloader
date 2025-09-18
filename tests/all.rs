use elf_loader::{Elf, load, load_dylib, load_exec};
use std::{collections::HashMap, fs::File, io::Read};

#[test]
fn relocate_dylib() {
    fn print(s: &str) {
        println!("{}", s);
    }

    let mut map = HashMap::new();
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    let liba = load_dylib!("target/liba.so").unwrap();
    let libb = load_dylib!("target/libb.so").unwrap();
    let libc = load_dylib!("target/libc.so").unwrap();
    let a = liba.easy_relocate([].iter(), &pre_find).unwrap();
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
    let b = libb.easy_relocate([&a].into_iter(), &pre_find).unwrap();
    let f = unsafe { b.get::<fn() -> i32>("b").unwrap() };
    assert!(f() == 2);
    let c = libc.easy_relocate([&b].into_iter(), &pre_find).unwrap();
    let f = unsafe { c.get::<fn() -> i32>("c").unwrap() };
    assert!(f() == 3);
}

#[test]
fn lazy_binding() {
    use std::sync::Arc;

    fn print(s: &str) {
        println!("{}", s);
    }
    let mut map = HashMap::new();
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    let liba = load_dylib!("target/liba.so").unwrap();
    let libb = load_dylib!("target/libb.so", lazy : true).unwrap();
    let a = liba.easy_relocate([].iter(), &pre_find).unwrap();
    let b = libb
        .relocate(
            [&a],
            &pre_find,
            &mut |_, _, _| Err(Box::new(())),
            Some(Arc::new(|name| unsafe {
                a.get::<()>(name)
                    .map(|sym| sym.into_raw())
                    .or_else(|| pre_find(name))
            })),
        )
        .unwrap();
    let f = unsafe { b.get::<fn() -> i32>("b").unwrap() };
    assert!(f() == 2);
}

#[test]
fn load_from_memory() {
    let mut file = File::open("target/liba.so").unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    let liba = load_dylib!("target/liba.so", &bytes).unwrap();
    let a = liba.easy_relocate([].iter(), &|_| None).unwrap();
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
}

#[test]
fn wrong_name_fails() {
    let _ = load_dylib!("target/this_location_is_definitely_non existent:^~")
        .err()
        .unwrap();
}

#[test]
fn type_mismatch() {
    let _ = load_exec!("target/liba.so").err().unwrap();
}

#[test]
fn load_elf() {
    let liba = load!("target/liba.so").unwrap();
    assert!(matches!(liba, Elf::Dylib(_)));
    let a = liba
        .easy_relocate([].into_iter(), &|_| None)
        .unwrap()
        .into_dylib()
        .unwrap();
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
}

#[test]
fn missing_symbol_fails() {
    let lib = load_dylib!("target/liba.so")
        .unwrap()
        .easy_relocate([].into_iter(), &|_| None)
        .unwrap();
    unsafe {
        assert!(lib.get::<*mut ()>("test_does_not_exist").is_none());
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug)]
struct S {
    a: u64,
    b: u32,
    c: u16,
    d: u8,
}

#[test]
fn test_id_struct() {
    let lib = load_dylib!("target/liba.so")
        .unwrap()
        .easy_relocate([].into_iter(), &|_| None)
        .unwrap();
    unsafe {
        let f = lib
            .get::<unsafe extern "C" fn(S) -> S>("test_identity_struct")
            .unwrap();
        assert_eq!(
            S {
                a: 1,
                b: 2,
                c: 3,
                d: 4
            },
            f(S {
                a: 1,
                b: 2,
                c: 3,
                d: 4
            })
        );
    }
}

#[test]
fn test_mini_loader() {
    use std::path::Path;
    use std::process::Command;

    let path = "target/mini-loader";
    if !Path::new(path).exists() {
        panic!("mini-loader binary not found at {}", path);
    }

    let exec_path = "target/exec_a";
    if !Path::new(exec_path).exists() {
        panic!("Test executable not found at {}", exec_path);
    }

    let mut cmd = Command::new(path);
    cmd.arg(exec_path);

    assert!(
        cmd.status()
            .expect("mini-loader could't load exec files!")
            .success()
    );
}
