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

    // Expose the output directory to tests
    println!("cargo:rustc-env=TEST_ARTIFACTS={}", out_dir.display());

    // Build steps for fixtures have been simplified: all non-test fixtures moved
    // to `examples/fixtures`. Only `exec_a` is built here for tests. Examples
    // should build their own fixtures from `examples/fixtures` as needed.

    // Re-create small set of runtime fixtures required by doctests/examples
    // (liba/libb/libc) from `examples/fixtures/rust` so doctests that load
    // `liba.so` continue to work.
    let rust_dylibs = [("liba", "a"), ("libb", "b"), ("libc", "c")];
    for (filename, crate_name) in &rust_dylibs {
        let src = format!("examples/fixtures/{}.rs", filename);
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
            .arg(&out_dir)
            .arg("-C")
            .arg("linker=rust-lld");

        let status = cmd.status().expect("Failed to run rustc");
        assert!(status.success(), "Failed to compile {}", filename);
    }

    // Get the compiler/linker to use
    let cc_target = if target == "aarch64-unknown-none" {
        "aarch64-unknown-linux-gnu"
    } else {
        &target
    };
    let compiler = cc::Build::new().target(cc_target).get_compiler();
    let cc_path = compiler.path();

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
