use gen_relocs::{Arch, DylibWriter, RelocEntry, SymbolDesc};

fn get_arch() -> Arch {
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
    } else if cfg!(target_arch = "x86") {
        Arch::X86
    } else if cfg!(target_arch = "loongarch64") {
        Arch::Loongarch64
    } else {
        panic!("Unsupported architecture for dynamic linking test");
    }
}

#[test]
fn gen_elf() {
    let arch = get_arch();
    let writer = DylibWriter::new(arch);

    // callee: returns 42
    // x86_64: mov eax, 42; ret  => b8 2a 00 00 00 c3
    // aarch64: mov w0, #42; ret => 40 05 80 52 c0 03 5f d6
    let callee_code = if arch == Arch::X86_64 {
        vec![0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3]
    } else if arch == Arch::Aarch64 {
        vec![0x40, 0x05, 0x80, 0x52, 0xc0, 0x03, 0x5f, 0xd6]
    } else if arch == Arch::X86 {
        vec![0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3]
    } else if arch == Arch::Riscv64 || arch == Arch::Riscv32 {
        vec![0x13, 0x05, 0xa0, 0x02, 0x67, 0x80, 0x00, 0x00]
    } else if arch == Arch::Arm {
        vec![0x2a, 0x00, 0xa0, 0xe3, 0x1e, 0xff, 0x2f, 0xe1]
    } else if arch == Arch::Loongarch64 {
        // 1. addi.w $a0, $zero, 42  => 04 2a 80 02
        //    (Put 42 into return register $a0/r4)
        // 2. jirl $zero, $ra, 0     => 20 00 00 4c
        //    (Return to address in $ra/r1)
        vec![0x04, 0xa8, 0x80, 0x02, 0x20, 0x00, 0x00, 0x4c]
    } else {
        panic!("Unsupported architecture");
    };

    // We need a jump slot relocation for "callee"
    // In gen_relocs, RelocEntry::with_name(name, REL_JUMP_SLOT) will create a PLT entry
    // and a JUMP_SLOT relocation in .rela.plt
    let (rel_jump_slot, rel_irelative) = if arch == Arch::X86_64 {
        (
            object::elf::R_X86_64_JUMP_SLOT,
            object::elf::R_X86_64_IRELATIVE,
        )
    } else if arch == Arch::Aarch64 {
        (
            object::elf::R_AARCH64_JUMP_SLOT,
            object::elf::R_AARCH64_IRELATIVE,
        )
    } else if arch == Arch::X86 {
        (object::elf::R_386_JMP_SLOT, object::elf::R_386_IRELATIVE)
    } else if arch == Arch::Riscv64 {
        (
            object::elf::R_RISCV_JUMP_SLOT,
            object::elf::R_RISCV_IRELATIVE,
        )
    } else if arch == Arch::Riscv32 {
        (
            object::elf::R_RISCV_JUMP_SLOT,
            object::elf::R_RISCV_IRELATIVE,
        )
    } else if arch == Arch::Arm {
        (object::elf::R_ARM_JUMP_SLOT, object::elf::R_ARM_IRELATIVE)
    } else if arch == Arch::Loongarch64 {
        (
            object::elf::R_LARCH_JUMP_SLOT,
            object::elf::R_LARCH_IRELATIVE,
        )
    } else {
        panic!("Unsupported architecture");
    };

    let symbols = vec![SymbolDesc::global_func("callee", &callee_code)];

    let relocs = vec![
        RelocEntry::with_name("callee", rel_jump_slot),
        RelocEntry::new(rel_irelative),
    ];

    let output = writer
        .write(&relocs, &symbols)
        .expect("Failed to generate ELF");

    // Use a unique name to avoid conflicts
    let mut elf_path = std::env::current_dir().expect("Failed to get current dir");
    elf_path.push(format!("test_plt_libloading_{:?}.so", arch));
    let elf_path_str = elf_path
        .to_str()
        .expect("Failed to convert path to string")
        .to_string();
    std::fs::write(&elf_path, &output.data).expect("Failed to write ELF to file");

    println!("Loading library from {}", elf_path_str);
    println!(
        "File exists: {}",
        std::path::Path::new(&elf_path_str).exists()
    );
    let lib = unsafe {
        let lib = libloading::os::unix::Library::open(
            Some(&elf_path_str),
            libloading::os::unix::RTLD_LAZY,
        )
        .expect("Failed to load library with libloading");
        libloading::Library::from(lib)
    };

    println!("Library loaded, getting callee...");
    // Test callee directly first
    let callee_abs_addr = unsafe {
        let func: libloading::Symbol<unsafe extern "C" fn() -> i32> =
            lib.get(b"callee").expect("Failed to get callee function");
        let addr = *func as *const () as u64;
        println!("Calling callee at {:#x}...", addr);
        let result = func();
        println!("Callee result: {}", result);
        assert_eq!(result, 42, "Direct call returned wrong value");
        addr
    };

    // Calculate load bias using __ifunc_resolver which is at text_vaddr
    let resolver_abs_addr = unsafe {
        let func: libloading::Symbol<unsafe extern "C" fn() -> u64> = lib
            .get(b"__ifunc_resolver")
            .expect("Failed to get resolver");
        *func as *const () as u64
    };
    let load_bias = resolver_abs_addr - output.text_vaddr;
    println!("Load bias: {:#x}", load_bias);

    // Find the PLT relocation for "callee"
    // Since we only have one relocation in our test, it's easy to find.
    // In a more complex case, we'd need to map sym_idx back to name.
    let plt_reloc = output
        .relocations
        .iter()
        .find(|r| r.r_type == rel_jump_slot)
        .expect("Failed to find PLT relocation");

    let got_entry_addr = load_bias + plt_reloc.vaddr;
    println!("GOT entry address for callee: {:#x}", got_entry_addr);

    let read = || unsafe {
        if std::mem::size_of::<usize>() == 8 {
            *(got_entry_addr as *const u64)
        } else {
            *(got_entry_addr as *const u32) as u64
        }
    };
    let got_val_before = read();
    // The helper function name is "callee@helper"
    // This helper function is generated by gen_relocs and it just calls callee@plt
    let helper_name = "callee@helper";
    let func: libloading::Symbol<extern "C" fn() -> i32> = unsafe {
        lib.get(helper_name.as_bytes())
            .expect("Failed to get helper function")
    };

    println!("Calling helper...");
    let result = func();
    assert_eq!(result, 42, "PLT call returned wrong value");
    let got_val_after = read();

    assert_eq!(
        got_val_after, callee_abs_addr,
        "GOT entry should be updated to callee address"
    );
    assert!(
        got_val_before != got_val_after,
        "GOT entry should change after PLT call"
    );
}
