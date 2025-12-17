use gen_relocs::{
    arch, 
    common::RelocEntry, 
    Arch
};
use elf::abi::*;
use object::write::RelocationFlags;

#[test]
fn test_x86_64_plt_generation() {
    // Create some test relocations with PLT references
    let relocs = vec![
        RelocEntry::new(
            "external_func",
            RelocationFlags::Elf {
                r_type: R_X86_64_PLT32,
            },
        ),
        RelocEntry::new(
            "another_func",
            RelocationFlags::Elf {
                r_type: R_X86_64_JUMP_SLOT,
            },
        ),
    ];

    let (plt_data, plt_map) = arch::generate_plt_for_arch(
        Arch::X86_64,
        0,
        &relocs,
        0x100,
        3,  // next_got_index
    );

    // Verify PLT was generated
    assert!(!plt_data.is_empty());
    assert!(plt_data.len() >= 16); // At least PLT[0]

    // Verify entries were created for both functions
    assert!(plt_map.contains_key("external_func"));
    assert!(plt_map.contains_key("another_func"));

    // PLT[0] should be at offset 0
    assert!(*plt_map.get("external_func").unwrap() > 0);
    assert!(*plt_map.get("another_func").unwrap() > 0);
}

#[test]
fn test_aarch64_plt_generation() {
    let relocs = vec![
        RelocEntry::new(
            "malloc",
            RelocationFlags::Elf {
                r_type: R_AARCH64_JUMP_SLOT,
            },
        ),
    ];

    let (plt_data, plt_map) = arch::generate_plt_for_arch(
        Arch::Aarch64,
        0,
        &relocs,
        0x100,
        3,
    );

    assert!(!plt_data.is_empty());
    assert!(plt_map.contains_key("malloc"));
}

#[test]
fn test_arm_plt_generation() {
    let relocs = vec![
        RelocEntry::new(
            "printf",
            RelocationFlags::Elf {
                r_type: R_ARM_JUMP_SLOT,
            },
        ),
    ];

    let (plt_data, plt_map) = arch::generate_plt_for_arch(
        Arch::Arm,
        0,
        &relocs,
        0x100,
        3,
    );

    assert!(!plt_data.is_empty());
    assert!(plt_map.contains_key("printf"));
}

#[test]
fn test_riscv64_plt_generation() {
    let relocs = vec![
        RelocEntry::new(
            "free",
            RelocationFlags::Elf {
                r_type: R_RISCV_JUMP_SLOT,
            },
        ),
    ];

    let (plt_data, plt_map) = arch::generate_plt_for_arch(
        Arch::Riscv64,
        0,
        &relocs,
        0x100,
        3,
    );

    assert!(!plt_data.is_empty());
    assert!(plt_map.contains_key("free"));
}

#[test]
fn test_riscv32_plt_generation() {
    let relocs = vec![
        RelocEntry::new(
            "puts",
            RelocationFlags::Elf {
                r_type: R_RISCV_JUMP_SLOT,
            },
        ),
    ];

    let (plt_data, plt_map) = arch::generate_plt_for_arch(
        Arch::Riscv32,
        0,
        &relocs,
        0x100,
        3,
    );

    assert!(!plt_data.is_empty());
    assert!(plt_map.contains_key("puts"));
}

#[test]
fn test_plt_generation_no_plt_relocs() {
    // Test with relocs that don't require PLT entries
    let relocs = vec![
        RelocEntry::new(
            "some_var",
            RelocationFlags::Elf {
                r_type: R_X86_64_RELATIVE,
            },
        ),
    ];

    let (plt_data, plt_map) = arch::generate_plt_for_arch(
        Arch::X86_64,
        0,
        &relocs,
        0x100,
        3,
    );

    // Should still generate minimal PLT[0]
    assert!(!plt_data.is_empty());
    // No external functions to add
    assert_eq!(plt_map.len(), 0);
}

#[test]
fn test_multiple_architectures() {
    let relocs = vec![
        RelocEntry::new(
            "func1",
            RelocationFlags::Elf {
                r_type: R_X86_64_PLT32,
            },
        ),
    ];

    for arch in &[
        Arch::X86_64,
        Arch::Aarch64,
        Arch::Arm,
        Arch::Riscv64,
        Arch::Riscv32,
    ] {
        let (plt_data, _) = arch::generate_plt_for_arch(*arch, 0, &relocs, 0x100, 3);
        assert!(!plt_data.is_empty(), "PLT should be generated for {:?}", arch);
    }
}
