#[cfg(all(feature = "fs", feature = "mmap"))]
mod fs {
    use elf_loader::{Elf, load, load_dylib, load_exec};
    use std::path::PathBuf;
    use std::{collections::HashMap, fs::File, io::Read};

    const TARGET_DIR: Option<&'static str> = option_env!("CARGO_TARGET_DIR");
    const TARGET_TMPDIR: Option<&'static str> = option_env!("CARGO_TARGET_TMPDIR");

    fn lib_path() -> std::path::PathBuf {
        TARGET_TMPDIR
            .unwrap_or(TARGET_DIR.unwrap_or("target"))
            .into()
    }

    const FILE_NAME: [&str; 3] = ["a.rs", "b.rs", "c.rs"];
    const DIR: &'static str = "example_dylib";

    fn compile() {
        static ONCE: ::std::sync::Once = ::std::sync::Once::new();
        ONCE.call_once(|| {
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
            let dir = PathBuf::from(DIR);
            for name in FILE_NAME {
                let mut cmd = ::std::process::Command::new(&rustc);
                let path = dir.join(name);
                cmd.arg(path)
                    .arg("-C")
                    .arg("panic=abort")
                    .arg("-C")
                    .arg("opt-level=3")
                    .arg("--out-dir")
                    .arg(lib_path());
                assert!(
                    cmd.status()
                        .expect("could not compile the test helpers!")
                        .success()
                );
            }
        });
    }

    #[test]
    fn relocate_dylib() {
        compile();
        fn print(s: &str) {
            println!("{}", s);
        }

        let mut map = HashMap::new();
        map.insert("print", print as _);
        let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
        let liba = load_dylib!(lib_path().join("liba.so").to_str().unwrap()).unwrap();
        let libb = load_dylib!(lib_path().join("libb.so").to_str().unwrap()).unwrap();
        let libc = load_dylib!(lib_path().join("libc.so").to_str().unwrap()).unwrap();
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
    fn load_from_memory() {
        compile();
        let mut file = File::open(lib_path().join("liba.so").to_str().unwrap()).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        let liba = load_dylib!("liba.so", &bytes).unwrap();
        let a = liba.easy_relocate([].iter(), &|_| None).unwrap();
        let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
        assert!(f() == 1);
    }

    #[test]
    fn wrong_name_fails() {
        compile();
        let _ = load_dylib!("target/this_location_is_definitely_non existent:^~")
            .err()
            .unwrap();
    }

    #[test]
    fn type_mismatch() {
        compile();
        let _ = load_exec!(lib_path().join("liba.so").to_str().unwrap())
            .err()
            .unwrap();
    }

    #[test]
    fn load_elf() {
        compile();
        let liba = load!(lib_path().join("liba.so").to_str().unwrap()).unwrap();
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
        compile();
        let lib = load_dylib!(lib_path().join("liba.so").to_str().unwrap())
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
        compile();
        let lib = load_dylib!(lib_path().join("liba.so").to_str().unwrap())
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
}
