use elf_loader::load_dylib;
use std::collections::HashMap;
use std::path::PathBuf;

const TARGET_DIR: Option<&'static str> = option_env!("CARGO_TARGET_DIR");

fn lib_path() -> PathBuf {
    let path: PathBuf = TARGET_DIR.unwrap_or("target").into();
    path.join("release")
}

const PACKAGE_NAME: [&str; 3] = ["a", "b", "c"];

fn compile() {
    static ONCE: ::std::sync::Once = ::std::sync::Once::new();
    ONCE.call_once(|| {
        for name in PACKAGE_NAME {
            let mut cmd = ::std::process::Command::new("cargo");
            cmd.arg("rustc")
                .arg("-r")
                .arg("-p")
                .arg(name)
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

fn main() {
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
