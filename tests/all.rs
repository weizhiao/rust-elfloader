use elf_loader::{
    Elf, Loader, Relocatable, load, load_dylib, load_exec, mmap::MmapImpl, object::ElfFile,
};
use std::{collections::HashMap, fs::File, io::Read, sync::Arc};

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
    let a = liba.relocator().pre_find(&pre_find).run().unwrap();
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
    let b = libb
        .relocator()
        .pre_find(&pre_find)
        .scope([&a].into_iter())
        .run()
        .unwrap();
    let f = unsafe { b.get::<fn() -> i32>("b").unwrap() };
    assert!(f() == 2);
    let c = libc
        .relocator()
        .pre_find(&pre_find)
        .scope([&b].into_iter())
        .run()
        .unwrap();
    let f = unsafe { c.get::<fn() -> i32>("c").unwrap() };
    assert!(f() == 3);
}

#[test]
fn lazy_binding() {
    fn print(s: &str) {
        println!("{}", s);
    }
    let mut map = HashMap::new();
    map.insert("print", print as *const () as usize);
    let map = Arc::new(map);
    let pre_find = Arc::new(move |name: &str| -> Option<*const ()> {
        map.get(name).copied().map(|p| p as *const ())
    });
    let liba = load_dylib!("target/liba.so").unwrap();
    let libb = load_dylib!("target/libb.so").unwrap();
    let a = liba.relocator().pre_find(&*pre_find).run().unwrap();
    let pre_find_clone = pre_find.clone();
    let b = libb
        .relocator()
        .pre_find(pre_find.clone())
        .scope([&a])
        .lazy(true)
        .lazy_scope(Some(Arc::new(move |name| pre_find_clone(name))))
        .run()
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
    let a = liba.relocator().run().unwrap();
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
    let a = liba.relocator().run().unwrap().into_dylib().unwrap();
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
}

#[test]
fn missing_symbol_fails() {
    let lib = load_dylib!("target/liba.so")
        .unwrap()
        .relocator()
        .run()
        .unwrap();
    let f = unsafe { lib.get::<fn() -> i32>("b") };
    assert!(f.is_none());
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
        .relocator()
        .run()
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
    map.insert("print", print as *const () as usize);
    let pre_find = Arc::new(move |name: &str| -> Option<*const ()> {
        map.get(name).copied().map(|p| p as *const ())
    });
    let mut loader = Loader::<MmapImpl>::new();
    let object = ElfFile::from_path("target/a.o").unwrap();
    let a = loader
        .load_relocatable(object)
        .unwrap()
        .relocator()
        .pre_find(pre_find.clone())
        .run()
        .unwrap();
    let b = loader
        .load_relocatable(ElfFile::from_path("target/b.o").unwrap())
        .unwrap()
        .relocator()
        .pre_find(pre_find.clone())
        .scope([&a])
        .run()
        .unwrap();
    let c = loader
        .load_relocatable(ElfFile::from_path("target/c.o").unwrap())
        .unwrap()
        .relocator()
        .pre_find(pre_find.clone())
        .scope([&a, &b])
        .run()
        .unwrap();
    let f = unsafe { a.get::<extern "C" fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
    let f = unsafe { b.get::<extern "C" fn() -> i32>("b").unwrap() };
    assert!(f() == 2);
    let f = unsafe { c.get::<extern "C" fn() -> i32>("c").unwrap() };
    assert!(f() == 3);
}

#[test]
#[cfg(target_arch = "x86_64")]
fn test_relocatable() {
    extern "C" fn external_func_impl() -> i32 {
        100
    }
    static EXTERNAL_VAR_IMPL: i32 = 200;

    let mut map = HashMap::new();
    map.insert("external_func", external_func_impl as *const () as usize);
    map.insert("external_var", &EXTERNAL_VAR_IMPL as *const _ as usize);
    map.insert("external_var_32", 0x1000 as usize);

    let pre_find = Arc::new(move |name: &str| -> Option<*const ()> {
        map.get(name).copied().map(|p| p as *const ())
    });

    let mut loader = Loader::<MmapImpl>::new();
    let object = ElfFile::from_path("target/x86_64.o").unwrap();
    let lib = loader
        .load_relocatable(object)
        .unwrap()
        .relocator()
        .pre_find(pre_find.clone())
        .run()
        .unwrap();

    unsafe {
        let asm_test_func = lib.get::<extern "C" fn() -> i32>("asm_test_func").unwrap();
        let result = asm_test_func();
        assert_eq!(result, 0x1156);
    }
}
