#[cfg(all(feature = "fs", feature = "mmap"))]
mod fs {
    use elf_loader::{Elf, load, load_dylib, load_exec};
    use std::env::consts;
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::{collections::HashMap, fs::File, io::Read};

    const TARGET_DIR: Option<&'static str> = option_env!("CARGO_TARGET_DIR");
    static TARGET_TRIPLE: OnceLock<String> = OnceLock::new();

    fn lib_path(file_name: &str) -> String {
        let path: PathBuf = TARGET_DIR.unwrap_or("target").into();
        path.join(TARGET_TRIPLE.get().unwrap())
            .join("release")
            .join(file_name)
            .to_str()
            .unwrap()
            .to_string()
    }

    const PACKAGE_NAME: [&str; 3] = ["a", "b", "c"];

    fn compile() {
        static ONCE: ::std::sync::Once = ::std::sync::Once::new();
        ONCE.call_once(|| {
            let arch = consts::ARCH;
            if arch.contains("x86_64") {
                TARGET_TRIPLE
                    .set("x86_64-unknown-linux-gnu".to_string())
                    .unwrap();
            } else if arch.contains("x86") {
                TARGET_TRIPLE
                    .set("i586-unknown-linux-gnu".to_string())
                    .unwrap();
            } else if arch.contains("arm") {
                TARGET_TRIPLE
                    .set("arm-unknown-linux-gnueabihf".to_string())
                    .unwrap();
            } else if arch.contains("riscv64") {
                TARGET_TRIPLE
                    .set("riscv64gc-unknown-linux-gnu".to_string())
                    .unwrap();
            } else if arch.contains("aarch64") {
                TARGET_TRIPLE
                    .set("aarch64-unknown-linux-gnu".to_string())
                    .unwrap();
            } else if arch.contains("loongarch64") {
                TARGET_TRIPLE
                    .set("loongarch64-unknown-linux-musl".to_string())
                    .unwrap();
            } else {
                unimplemented!()
            }

            for name in PACKAGE_NAME {
                let mut cmd = ::std::process::Command::new("cargo");
                cmd.arg("rustc")
                    .arg("-r")
                    .arg("-p")
                    .arg(name)
                    .arg("--target")
                    .arg(TARGET_TRIPLE.get().unwrap().as_str())
                    .arg("--")
                    .arg("-C")
                    .arg("panic=abort");
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
        let liba = load_dylib!(&lib_path("liba.so")).unwrap();
        let libb = load_dylib!(&lib_path("libb.so")).unwrap();
        let libc = load_dylib!(&lib_path("libc.so")).unwrap();
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
        compile();
        fn print(s: &str) {
            println!("{}", s);
        }
        let mut map = HashMap::new();
        map.insert("print", print as _);
        let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
        let liba = load_dylib!(&lib_path("liba.so")).unwrap();
        let libb = load_dylib!(&lib_path("libb.so"), lazy : true).unwrap();
        let a = liba.easy_relocate([].iter(), &pre_find).unwrap();
        let b = libb
            .relocate(
                [&a].into_iter(),
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
        compile();
        let mut file = File::open(&lib_path("liba.so")).unwrap();
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
        let _ = load_exec!(&lib_path("liba.so")).err().unwrap();
    }

    #[test]
    fn load_elf() {
        compile();
        let liba = load!(&lib_path("liba.so")).unwrap();
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
        let lib = load_dylib!(&lib_path("liba.so"))
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
        let lib = load_dylib!(&lib_path("liba.so"))
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
