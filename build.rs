use std::{env, str};
const DYLIB_FILE_NAME: [&str; 3] = ["liba.rs", "libb.rs", "libc.rs"];
const DYLIB_DIR_PATH: &str = "test-dylib";

const EXEC_FILE_NAME: [&str; 1] = ["exec_a.rs"];
const EXEC_DIR_PATH: &str = "test-exec";

fn compile_dylib(target: &String) {
    for name in DYLIB_FILE_NAME {
        let mut cmd = ::std::process::Command::new("rustc");
        cmd.arg("-O")
            .arg("--target")
            .arg(target)
            .arg("-C")
            .arg("panic=abort")
            .arg("-C")
            .arg("linker=lld")
            .arg(format!("{}/{}", DYLIB_DIR_PATH, name))
            .arg("--out-dir")
            .arg("target");
        assert!(
            cmd.status()
                .expect("could not compile the dylibs!")
                .success()
        );
    }
}

fn compile_exec(target: &String) {
    for name in EXEC_FILE_NAME {
        let mut cmd = ::std::process::Command::new("rustc");
        cmd.arg("-O")
            .arg("--target")
            .arg(target)
            .arg("-C")
            .arg("target-feature=+crt-static")
            .arg("-C")
            .arg("panic=abort")
            .arg(format!("{}/{}", EXEC_DIR_PATH, name))
            .arg("--out-dir")
            .arg("target");
        assert!(
            cmd.status()
                .expect("could not compile the executables!")
                .success()
        );
    }
}

fn compile_mini_loader(target: &String){
    let mut cmd = ::std::process::Command::new("cargo");
    cmd.arg("build")
        .arg("-p")
        .arg("mini-loader")
        .arg("--target")
        .arg(target)
        .arg("-Z")
        .arg("unstable-options")
        .arg("--artifact-dir")
        .arg("target");

    assert!(
        cmd.status()
            .expect("could not compile the mini-loader!")
            .success()
    );
}

fn main() {
    let ci = env::var("ELF_LOADER_CI").is_ok();
    if ci {
        println!("cargo:rerun-if-changed=always_trigger_rebuild");
        let target = env::var("TARGET").unwrap();
        compile_dylib(&target);
        compile_exec(&target);
        compile_mini_loader(&target);
    }
}
