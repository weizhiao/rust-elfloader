use elf_loader::load_dylib;
use std::collections::HashMap;
use std::path::PathBuf;

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
