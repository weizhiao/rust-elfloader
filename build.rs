use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let fixture_path = PathBuf::from("tests/fixtures");
    if !fixture_path.exists() {
        return;
    }

    println!("cargo:rerun-if-changed=tests/fixtures");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();

    // Get the compiler/linker to use
    let compiler = cc::Build::new().target(&target).get_compiler();
    let cc_path = compiler.path();

    // Expose the output directory to tests
    println!("cargo:rustc-env=TEST_ARTIFACTS={}", out_dir.display());

    // 1. Compile Rust Dylibs (liba, libb, libc)
    // (filename, crate_name)
    let rust_dylibs = [("liba", "a"), ("libb", "b"), ("libc", "c")];

    for (filename, crate_name) in &rust_dylibs {
        let src = format!("tests/fixtures/rust/{}.rs", filename);
        let mut cmd = Command::new("rustc");
        cmd.arg(&src)
            .arg("--crate-type=cdylib")
            .arg("--crate-name")
            .arg(crate_name)
            .arg("--target")
            .arg(&target)
            .arg("-O")
            .arg("-C")
            .arg("panic=abort")
            .arg("--out-dir")
            .arg(&out_dir);

        if target.contains("unknown-none") {
            cmd.arg("-C").arg("linker=rust-lld");
        } else {
            cmd.arg("-C").arg(format!("linker={}", cc_path.display()));
        }

        let status = cmd.status().expect("Failed to run rustc");
        assert!(status.success(), "Failed to compile {}", filename);
    }

    // 2. Compile Rust Relocatable Objects
    for (filename, crate_name) in &rust_dylibs {
        let src = format!("tests/fixtures/rust/{}.rs", filename);
        let mut cmd = Command::new("rustc");
        cmd.arg(&src)
            .arg("--emit=obj")
            .arg("--crate-name")
            .arg(crate_name)
            .arg("--target")
            .arg(&target)
            .arg("-O")
            .arg("-C")
            .arg("panic=abort")
            .arg("--out-dir")
            .arg(&out_dir);

        if target.contains("unknown-none") {
            cmd.arg("-C").arg("linker=rust-lld");
        } else {
            cmd.arg("-C").arg(format!("linker={}", cc_path.display()));
        }

        let status = cmd.status().expect("Failed to run rustc");
        assert!(status.success(), "Failed to compile object {}", filename);
    }

    // x86_64 specific relocatable
    if target.contains("x86_64") {
        let src = "tests/fixtures/rust/x86_64.rs";
        let mut cmd = Command::new("rustc");
        cmd.arg(src)
            .arg("--emit=obj")
            .arg("--crate-name")
            .arg("x86_64")
            .arg("--target")
            .arg(&target)
            .arg("-O")
            .arg("-C")
            .arg("panic=abort")
            .arg("--out-dir")
            .arg(&out_dir);

        if target.contains("unknown-none") {
            cmd.arg("-C").arg("linker=rust-lld");
        } else {
            cmd.arg("-C").arg(format!("linker={}", cc_path.display()));
        }

        let status = cmd.status().expect("Failed to run rustc");
        assert!(status.success(), "Failed to compile x86_64 object");
    }

    // 3. Compile C Dylibs (libfoo, libbar) using cc
    // Compile libfoo.so
    let foo_c = "tests/fixtures/c/foo.c";
    let foo_so = out_dir.join("libfoo.so");
    let mut cmd = Command::new(cc_path);
    cmd.arg(foo_c)
        .arg("-shared")
        .arg("-fPIC")
        .arg("-o")
        .arg(&foo_so);

    for arg in compiler.args() {
        cmd.arg(arg);
    }

    if let Ok(status) = cmd.status() {
        if status.success() {
            // Compile libbar.so (links against libfoo)
            let bar_c = "tests/fixtures/c/bar.c";
            let bar_so = out_dir.join("libbar.so");
            let mut cmd = Command::new(cc_path);
            cmd.arg(bar_c)
                .arg("-shared")
                .arg("-fPIC")
                .arg("-o")
                .arg(&bar_so)
                .arg("-L")
                .arg(&out_dir)
                .arg("-lfoo");

            for arg in compiler.args() {
                cmd.arg(arg);
            }
            let _ = cmd.status();
        }
    }

    // 4. Compile exec_a
    let exec_a_c = "tests/fixtures/c/exec_a.c";
    let exec_a = out_dir.join("exec_a");
    let mut cmd = Command::new(cc_path);
    cmd.arg(exec_a_c)
        .arg("-no-pie")
        .arg("-fno-pic")
        .arg("-o")
        .arg(&exec_a);

    for arg in compiler.args() {
        cmd.arg(arg);
    }
    let _ = cmd.status();

    // Copy exec_a to target/exec_a for mini-loader tests
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let target_dir = manifest_dir.join("target");
    if target_dir.exists() {
        let dest = target_dir.join("exec_a");
        let _ = std::fs::copy(&exec_a, &dest);
    }
}
