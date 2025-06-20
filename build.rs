use std::{env, str};
const FILE_NAME: [&str; 3] = ["liba.rs", "libb.rs", "libc.rs"];
const DIR_PATH: &str = "test-dylib";

fn compile(target: String) {
    for name in FILE_NAME {
        let mut cmd = ::std::process::Command::new("rustc");
        cmd.arg("-O")
            .arg("--target")
            .arg(&target)
            .arg("-C")
            .arg("panic=abort")
            .arg("-C")
            .arg("linker=lld")
            .arg(format!("{}/{}", DIR_PATH, name))
            .arg("--out-dir")
            .arg("target");
        assert!(
            cmd.status()
                .expect("could not compile the test helpers!")
                .success()
        );
    }
}

fn main() {
    let ci = env::var("ELF_LOADER_CI").is_ok();
    if ci {
        println!("cargo:rerun-if-changed=always_trigger_rebuild");
        let target = env::var("TARGET").unwrap();
        compile(target);
    }
}
