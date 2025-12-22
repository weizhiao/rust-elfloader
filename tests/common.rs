use gen_relocs::Arch;

pub fn get_arch() -> Arch {
    if cfg!(target_arch = "x86_64") {
        Arch::X86_64
    } else if cfg!(target_arch = "aarch64") {
        Arch::Aarch64
    } else if cfg!(target_arch = "riscv64") {
        Arch::Riscv64
    } else if cfg!(target_arch = "riscv32") {
        Arch::Riscv32
    } else if cfg!(target_arch = "arm") {
        Arch::Arm
    } else {
        panic!("Unsupported architecture for dynamic linking test");
    }
}

pub const EXTERNAL_FUNC_NAME: &str = "external_func";
pub const EXTERNAL_VAR_NAME: &str = "external_var";
pub const EXTERNAL_TLS_NAME: &str = "external_tls";
pub const LOCAL_VAR_NAME: &str = "local_var";
