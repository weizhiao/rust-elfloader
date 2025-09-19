use std::{env, str};
const DYLIB_FILE_NAME: [&str; 3] = ["liba.rs", "libb.rs", "libc.rs"];
const DYLIB_DIR_PATH: &str = "test-dylib";

const EXEC_FILE_NAME: [&str; 1] = ["exec_a.c"];
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
        let source_file = format!("{}/{}", EXEC_DIR_PATH, name);
        let output_name = name.strip_suffix(".c").unwrap();
        let output_path = format!("target/{}", output_name);

        let compiler = if target.starts_with("riscv") {
            "riscv64-linux-gnu-gcc"
        } else if target.starts_with("aarch64") {
            "aarch64-linux-gnu-gcc"
        } else if target.starts_with("x86_64") {
            "x86_64-linux-gnu-gcc"
        } else {
            return;
        };

        let mut cmd = ::std::process::Command::new(compiler);
        cmd.arg("-O2")
            .arg("-static")
            .arg(source_file)
            .arg("-o")
            .arg(output_path);

        assert!(
            cmd.status()
                .expect("could not compile the executables!")
                .success()
        );
    }
}

fn main() {
    let ci = env::var("ELF_LOADER_CI").is_ok();
    if ci {
        println!("cargo:rerun-if-changed=always_trigger_rebuild");
        let target = env::var("TARGET").unwrap();
        compile_dylib(&target);
        compile_exec(&target);
    }
}
