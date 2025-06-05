use elf_loader::load_dylib;
use std::collections::HashMap;
use std::env::consts;
use std::sync::OnceLock;

static TARGET_TRIPLE: OnceLock<String> = OnceLock::new();

const FILE_NAME: [&str; 3] = ["liba.rs", "libb.rs", "libc.rs"];
const DIR_PATH: &str = "test-dylib";

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
        } else if arch.contains("riscv32") {
            TARGET_TRIPLE
                .set("riscv32gc-unknown-linux-gnu".to_string())
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

        for name in FILE_NAME {
            let mut cmd = ::std::process::Command::new("rustc");
            cmd.arg("-O")
                .arg("--target")
                .arg(TARGET_TRIPLE.get().unwrap().as_str())
                .arg("-C")
                .arg("panic=abort")
                .arg("-C")
                .arg("link-args=-Wl,--pack-dyn-relocs=relr")
                .arg(format!("{}/{}", DIR_PATH, name))
                .arg("--out-dir")
                .arg("target");
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
    let liba = load_dylib!("target/liba.so").unwrap();
    let libb = load_dylib!("target/libb.so").unwrap();
    let libc = load_dylib!("target/libc.so").unwrap();
    let a = liba.easy_relocate([], &pre_find).unwrap();
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    assert!(f() == 1);
    let b = libb.easy_relocate([&a], &pre_find).unwrap();
    let f = unsafe { b.get::<fn() -> i32>("b").unwrap() };
    assert!(f() == 2);
    let c = libc.easy_relocate([&b], &pre_find).unwrap();
    let f = unsafe { c.get::<fn() -> i32>("c").unwrap() };
    assert!(f() == 3);
}
