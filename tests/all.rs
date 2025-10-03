use elf_loader::{
    Elf, Loader, RelocatedDylib, load, load_dylib, load_exec, mmap::MmapImpl, object::ElfFile,
};
use std::{collections::HashMap, fs::File, io::Read};

const EMPTY: &[RelocatedDylib; 0] = &[];

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
    let a = liba.easy_relocate(EMPTY, &pre_find).unwrap();
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
    let a = liba.easy_relocate(EMPTY, &pre_find).unwrap();
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
    let a = liba.easy_relocate(EMPTY, &|_| None).unwrap();
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
        .easy_relocate(EMPTY, &|_| None)
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
        .easy_relocate(EMPTY, &|_| None)
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
        .easy_relocate(EMPTY, &|_| None)
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

#[cfg(target_arch = "x86_64")]
#[test]
fn load_relocatable() {
    fn print(s: &str) {
        println!("{}", s);
    }

    let mut map = HashMap::new();
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    let mut loader = Loader::<MmapImpl>::new();
    let object = ElfFile::from_path("target/a.o").unwrap();
    let a = loader
        .load_relocatable(object, None)
        .unwrap()
        .relocate(&[], &pre_find)
        .unwrap();
    let b = loader
        .load_relocatable(ElfFile::from_path("target/b.o").unwrap(), None)
        .unwrap()
        .relocate(&[&a], &pre_find)
        .unwrap();
    let c = loader
        .load_relocatable(ElfFile::from_path("target/c.o").unwrap(), None)
        .unwrap()
        .relocate(&[&a, &b], &pre_find)
        .unwrap();
    let f = unsafe { a.get::<extern "C" fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
    let f = unsafe { b.get::<extern "C" fn() -> i32>("b").unwrap() };
    assert!(f() == 2);
    let f = unsafe { c.get::<extern "C" fn() -> i32>("c").unwrap() };
    assert!(f() == 3);
}
