pub(crate) fn generate_plt0_code() -> Vec<u8> {
    let mut code = vec![0; 32];
    // pcaddi $t0, ...
    // ld.d $t1, $t0, ... ; GOT[2]
    // ld.d $t0, $t0, ... ; GOT[1]
    // jirl $zero, $t1, 0
    code
}

pub(crate) fn patch_plt0(plt_data: &mut [u8], plt0_off: usize, plt0_vaddr: u64, got_vaddr: u64) {
    let pc = plt0_vaddr + 4;
    let target_got1 = got_vaddr + 8;
    let target_got2 = got_vaddr + 16;

    let off1 = (target_got1 as i64 - pc as i64);
    let off2 = (target_got2 as i64 - pc as i64);

    // pcaddi $t0, imm20
    let imm20 = (off1 >> 2) as i32;
    let pcaddi = 0x18000000u32 | (12 << 0) | ((imm20 as u32 & 0xfffff) << 5); // $t0 is $r12
    let move_idx = 0x001501a5u32; // or $a1, $t1, $zero (move $a1, $t1)
    let ld_dylib = 0x28c00000u32 | (4 << 0) | (12 << 5) | (((off2 & 0xfff) as u32) << 10); // $a0 is $r4
    let ld_resolver = 0x28c00000u32 | (13 << 0) | (12 << 5) | (((off1 & 0xfff) as u32) << 10); // $t1 is $r13
    let jirl = 0x4c000000u32 | (0 << 0) | (13 << 5) | (0 << 10); // jirl $zero, $t1, 0

    plt_data[plt0_off..plt0_off + 4].copy_from_slice(&pcaddi.to_le_bytes());
    plt_data[plt0_off + 4..plt0_off + 8].copy_from_slice(&move_idx.to_le_bytes());
    plt_data[plt0_off + 8..plt0_off + 12].copy_from_slice(&ld_dylib.to_le_bytes());
    plt_data[plt0_off + 12..plt0_off + 16].copy_from_slice(&ld_resolver.to_le_bytes());
    plt_data[plt0_off + 16..plt0_off + 20].copy_from_slice(&jirl.to_le_bytes());
}

pub(crate) fn generate_plt_entry_code(
    _got_idx: u64,
    reloc_idx: u32,
    plt_entry_offset: u64,
) -> Vec<u8> {
    let mut code = vec![0; 32];
    // pcaddi $t1, ...
    // ld.d $t1, $t1, ...
    // jirl $zero, $t1, 0
    // nop
    // li.d $t1, reloc_idx
    // b PLT0

    // li.d $t1, reloc_idx
    let li = 0x02800000 | (13 << 0) | ((reloc_idx & 0xfff) << 10); // addi.d $t1, $zero, imm

    // b PLT0
    let plt0_off = -(plt_entry_offset as i32 + 20);
    let b = 0x50000000 | (((plt0_off >> 2) as u32 & 0x3ffffff) << 0);

    code[16..20].copy_from_slice(&li.to_le_bytes());
    code[20..24].copy_from_slice(&b.to_le_bytes());
    code
}

pub(crate) fn patch_plt_entry(
    plt_data: &mut [u8],
    plt_entry_off: usize,
    plt_entry_vaddr: u64,
    target_got_vaddr: u64,
) {
    let pc = plt_entry_vaddr;
    let off = (target_got_vaddr as i64 - pc as i64);

    let imm20 = (off >> 2) as i32;
    let pcaddi = 0x18000000u32 | (13 << 0) | ((imm20 as u32 & 0xfffff) << 5); // $t1 is $r13
    let ld = 0x28c00000u32 | (13 << 0) | (13 << 5) | (((off & 0xfff) as u32) << 10);
    let jirl = 0x4c000000u32 | (0 << 0) | (13 << 5) | (0 << 10);

    plt_data[plt_entry_off..plt_entry_off + 4].copy_from_slice(&pcaddi.to_le_bytes());
    plt_data[plt_entry_off + 4..plt_entry_off + 8].copy_from_slice(&ld.to_le_bytes());
    plt_data[plt_entry_off + 8..plt_entry_off + 12].copy_from_slice(&jirl.to_le_bytes());
}

pub(crate) fn generate_helper_code() -> Vec<u8> {
    // bl <offset>
    // ret
    let mut code = vec![0; 8];
    code[0..4].copy_from_slice(&[0x00, 0x00, 0x00, 0x50]);
    code[4..8].copy_from_slice(&[0x00, 0x00, 0x2b, 0x4c]);
    code
}

pub(crate) fn patch_helper(
    text_data: &mut [u8],
    helper_text_off: usize,
    helper_vaddr: u64,
    target_plt_vaddr: u64,
) {
    let off = (target_plt_vaddr as i64 - helper_vaddr as i64) / 4;
    let insn = 0x50000000 | ((off & 0x3ffffff) as u32);
    text_data[helper_text_off..helper_text_off + 4].copy_from_slice(&insn.to_le_bytes());
}

pub(crate) fn get_ifunc_resolver_code() -> Vec<u8> {
    let mut code = vec![0; 16];
    // pcaddi $a0, 2
    // ld.d $a0, $a0, 0
    // jirl $zero, $ra, 0
    code[0..4].copy_from_slice(&[0x44, 0x00, 0x00, 0x18]);
    code[4..8].copy_from_slice(&[0x84, 0x00, 0xc0, 0x28]);
    code[8..12].copy_from_slice(&[0x00, 0x00, 0x2b, 0x4c]);
    code
}

pub(crate) fn patch_ifunc_resolver(
    text_data: &mut [u8],
    offset: usize,
    _resolver_vaddr: u64,
    target_vaddr: u64,
) {
    text_data[offset + 8..offset + 16].copy_from_slice(&target_vaddr.to_le_bytes());
}
