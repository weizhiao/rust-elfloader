mod common;

use common::get_path;
use elf_loader::{Loader, Relocatable, object::ElfFile};
use rstest::rstest;
use std::collections::HashMap;

#[rstest]
fn test_static_relocation() {
    // Try non-prefixed complex name first, fall back to arch-prefixed name
    let path = get_path("out_static.o");
    if !path.exists() {
        panic!("gen_relocs static fixture not found - test requires out_static.o");
    }

    extern "C" fn external_func() -> i32 {
        100
    }

    static EXTERNAL_VAR: i32 = 200;

    let mut map: HashMap<String, usize> = HashMap::new();
    map.insert(
        gen_relocs::EXTERNAL_FUNC_NAME.to_string(),
        external_func as *const () as usize,
    );
    map.insert(
        gen_relocs::EXTERNAL_VAR_NAME.to_string(),
        &EXTERNAL_VAR as *const i32 as usize,
    );

    let pre_map = map.clone();
    let pre_find = move |name: &str| -> Option<*const ()> {
        pre_map.get(name).copied().map(|p| p as *const ())
    };

    let mut loader = Loader::new();
    let obj = loader
        .load_relocatable(ElfFile::from_path(path.to_str().unwrap()).unwrap())
        .unwrap();
    let relocated = obj.relocator().symbols(&pre_find).relocate().unwrap();

    // Locate the base of the .data section via the `local_var` symbol
    // which the generator places at offset 0x20 within .data.
    let local_ptr = unsafe {
        relocated
            .get::<i32>("local_var")
            .expect("missing `local_var` in relocated object")
    };
    let local_addr = local_ptr.into_raw() as usize;
    let data_base = local_addr - gen_relocs::LOCAL_VAR_OFF;

    let external_func_addr = map.get("external_func").copied().unwrap();
    let external_var_addr = map.get("external_var").copied().unwrap();
    let data_section_addr = data_base;

    // Helpers
    unsafe fn read_u64(p: *const u8) -> u64 {
        let mut b = [0u8; 8];
        unsafe { core::ptr::copy_nonoverlapping(p, b.as_mut_ptr(), 8) };
        u64::from_le_bytes(b)
    }
    unsafe fn read_i32(p: *const u8) -> i32 {
        let mut b = [0u8; 4];
        unsafe { core::ptr::copy_nonoverlapping(p, b.as_mut_ptr(), 4) };
        i32::from_le_bytes(b)
    }

    // RELOC_OFF_ABS: R_X86_64_64 (Absolute) -> external_func + addend
    {
        let addr = (data_base + gen_relocs::RELOC_OFF_ABS) as *const u8;
        let val = unsafe { read_u64(addr) } as usize;
        assert_eq!(
            val,
            external_func_addr.wrapping_add(gen_relocs::RELOC_OFF_ABS),
            "RELOC_OFF_ABS: R_X86_64_64 mismatch"
        );
    }

    // RELOC_OFF_GOT: R_X86_64_GOTPCREL -> GOT entry - P
    {
        let p = (data_base + gen_relocs::RELOC_OFF_GOT) as *const u8;
        let val = unsafe { read_i32(p) };
        // Target address = P + val
        let target = (p as usize).wrapping_add(val as usize);
        // Target should be a GOT entry containing external_var_addr
        let got_val = unsafe { read_u64(target as *const u8) } as usize;
        assert_eq!(
            got_val, external_var_addr,
            "0x18: R_X86_64_GOTPCREL GOT entry mismatch"
        );
    }

    // RELOC_OFF_PC_REL: R_X86_64_GOTPCREL -> GOT entry - P
    {
        let p = (data_base + gen_relocs::RELOC_OFF_PC_REL) as *const u8;
        let val = unsafe { read_i32(p) };
        // Target address = P + val
        let target = (p as usize).wrapping_add(val as usize);
        // Target should be a GOT entry containing external_func_addr
        let got_val = unsafe { read_u64(target as *const u8) } as usize;
        assert_eq!(
            got_val, external_func_addr,
            "0x20: R_X86_64_GOTPCREL GOT entry mismatch"
        );
    }

    // RELOC_OFF_PLT: R_X86_64_PLT32 -> PLT entry - P (or direct if close)
    {
        let p = (data_base + gen_relocs::RELOC_OFF_PLT) as *const u8;
        let val = unsafe { read_i32(p) };
        let target = (p as usize).wrapping_add(val as usize);

        // If it's close, it might be direct. If far, it's PLT.
        // Since we don't know for sure, we can check if it's EITHER:
        // 1. external_func_addr
        // 2. A PLT entry that jumps to external_func_addr (hard to verify without parsing PLT)

        // However, we know that if it's a PLT entry, it should be executable code.
        // And if we call it, it should work.
        // But we can't easily call it here safely.

        // Let's assume for now that if it's not external_func_addr, it's a PLT entry.
        if target != external_func_addr {
            // Verify it looks like a PLT entry?
            // PLT entry: endbr64 (f3 0f 1e fa) ...
            let code = unsafe { read_u64(target as *const u8) };
            // 0xfa1e0ff3 is little endian for f3 0f 1e fa
            // But wait, read_u64 reads 8 bytes.
            // First 4 bytes: f3 0f 1e fa
            if (code & 0xffffffff) == 0xfa1e0ff3 {
                println!("0x28: Verified PLT entry signature");
            } else {
                // It might not be endbr64 if not enabled?
                // But `src/arch/x86_64.rs` defines PLT_ENTRY with endbr64.
                panic!(
                    "0x28: Target {:#x} is neither function {:#x} nor valid PLT",
                    target, external_func_addr
                );
            }
        }
    }

    // RELOC_OFF_RELATIVE: R_X86_64_64 (Relative-like) -> data_base + 0x2000
    {
        let addr = (data_base + gen_relocs::RELOC_OFF_RELATIVE) as *const u8;
        let val = unsafe { read_u64(addr) } as usize;
        assert_eq!(
            val,
            data_section_addr.wrapping_add(0x2000),
            "RELOC_OFF_RELATIVE: R_X86_64_64 (Relative) mismatch"
        );
    }

    // RELOC_OFF_TLS: R_X86_64_64 (TLS-like) -> external_var
    {
        let addr = (data_base + gen_relocs::RELOC_OFF_TLS) as *const u8;
        let val = unsafe { read_u64(addr) } as usize;
        assert_eq!(
            val, external_var_addr,
            "RELOC_OFF_TLS: R_X86_64_64 (TLS) mismatch"
        );
    }
}
