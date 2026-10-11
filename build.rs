use std::env;

fn main() {
    for name in [
        "TARGET",
        "CARGO_CFG_TARGET_ABI",
        "CARGO_CFG_TARGET_ARCH",
        "CARGO_FEATURE_LAZY_BINDING",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rustc-check-cfg=cfg(riscv_float_abi, values(\"soft\", \"f\", \"d\", \"q\"))");

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_arch != "riscv32" && target_arch != "riscv64" {
        return;
    }

    let target = env::var("TARGET").expect("Cargo must provide TARGET");
    let target_abi = env::var("CARGO_CFG_TARGET_ABI").unwrap_or_default();
    let float_abi = float_abi_from_target_abi(&target_abi)
        .or_else(|| float_abi_from_target(&target))
        .unwrap_or_else(|| {
            panic!(
                "cannot determine the floating-point calling ABI for {target} (target_abi={target_abi:?}); \
                 add an explicit ABI mapping in build.rs"
            )
        });
    assert!(
        float_abi != "q" || env::var_os("CARGO_FEATURE_LAZY_BINDING").is_none(),
        "RISC-V quad floating-point ABI is not supported by lazy-binding; disable that feature"
    );
    println!("cargo:rustc-cfg=riscv_float_abi=\"{float_abi}\"");
}

fn float_abi_from_target_abi(target_abi: &str) -> Option<&'static str> {
    match target_abi {
        "ilp32" | "ilp32e" | "lp64" | "lp64e" => Some("soft"),
        "ilp32f" | "lp64f" => Some("f"),
        "ilp32d" | "lp64d" => Some("d"),
        "ilp32q" | "lp64q" => Some("q"),
        _ => None,
    }
}

// Rust targets often leave target_abi empty. These are explicit mappings from
// their target specifications' llvm-abiname, not guesses from ISA extensions.
fn float_abi_from_target(target: &str) -> Option<&'static str> {
    match target {
        "riscv32e-unknown-none-elf"
        | "riscv32em-unknown-none-elf"
        | "riscv32emc-unknown-none-elf"
        | "riscv32i-unknown-none-elf"
        | "riscv32im-risc0-zkvm-elf"
        | "riscv32im-unknown-none-elf"
        | "riscv32ima-unknown-none-elf"
        | "riscv32imac-esp-espidf"
        | "riscv32imac-unknown-none-elf"
        | "riscv32imac-unknown-nuttx-elf"
        | "riscv32imac-unknown-xous-elf"
        | "riscv32imc-esp-espidf"
        | "riscv32imc-unknown-none-elf"
        | "riscv32imc-unknown-nuttx-elf"
        | "riscv64im-unknown-none-elf"
        | "riscv64imac-unknown-none-elf"
        | "riscv64imac-unknown-nuttx-elf" => Some("soft"),
        "riscv32imafc-esp-espidf"
        | "riscv32imafc-unknown-none-elf"
        | "riscv32imafc-unknown-nuttx-elf" => Some("f"),
        "riscv32-wrs-vxworks"
        | "riscv32gc-unknown-linux-gnu"
        | "riscv32gc-unknown-linux-musl"
        | "riscv64-linux-android"
        | "riscv64-wrs-vxworks"
        | "riscv64a23-unknown-linux-gnu"
        | "riscv64gc-unknown-freebsd"
        | "riscv64gc-unknown-fuchsia"
        | "riscv64gc-unknown-hermit"
        | "riscv64gc-unknown-linux-gnu"
        | "riscv64gc-unknown-linux-musl"
        | "riscv64gc-unknown-managarm-mlibc"
        | "riscv64gc-unknown-netbsd"
        | "riscv64gc-unknown-none-elf"
        | "riscv64gc-unknown-nuttx-elf"
        | "riscv64gc-unknown-openbsd"
        | "riscv64gc-unknown-redox" => Some("d"),
        _ => None,
    }
}
