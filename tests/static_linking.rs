mod common;

use common::get_path;
use elf_loader::{Loader, Relocatable, object::ElfFile};
use rstest::rstest;
use std::collections::HashMap;

#[rstest]
#[cfg(target_arch = "x86_64")]
fn test_static_relocation_x86_64() {
    let path = get_path("x86_64.o");
    if !path.exists() {
        eprintln!("Skipping test: x86_64.o not found");
        return;
    }

    extern "C" fn external_func() -> i32 {
        100
    }

    static EXTERNAL_VAR: i32 = 200;
    // Use a fixed address that fits in 32 bits for R_X86_64_32/32S relocation test.
    // The code only uses the address value, it doesn't dereference it.
    const EXTERNAL_VAR_32_ADDR: usize = 0x100000;

    let mut map = HashMap::new();
    map.insert("external_func", external_func as *const () as usize);
    map.insert("external_var", &EXTERNAL_VAR as *const i32 as usize);
    map.insert("external_var_32", EXTERNAL_VAR_32_ADDR);

    let pre_find =
        move |name: &str| -> Option<*const ()> { map.get(name).copied().map(|p| p as *const ()) };

    let mut loader = Loader::new();
    let obj = loader
        .load_relocatable(ElfFile::from_path(path.to_str().unwrap()).unwrap())
        .unwrap();
    let relocated = obj.relocator().symbols(&pre_find).relocate().unwrap();

    // Verify execution if possible, or just that relocation succeeded.
    // The object has 'asm_test_func'.
    let f = unsafe {
        *relocated
            .get::<extern "C" fn() -> usize>("asm_test_func")
            .unwrap()
    };

    // The function returns:
    // rax = external_var_32 (address) + (external_func() + external_var + local_var)
    // rax = EXTERNAL_VAR_32_ADDR + (100 + 200 + 42)
    // rax = 0x100000 + 342

    let result = f();
    let expected = EXTERNAL_VAR_32_ADDR + 342;
    assert_eq!(result, expected);
}
