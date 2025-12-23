pub(crate) fn generate_plt0_code() -> Vec<u8> {
    let mut code = vec![0; 32];
    // str lr, [sp, #-4]!
    // ldr lr, [pc, #4] ; GOT[1]
    // ldr pc, [pc, #4] ; GOT[2]
    code[0..4].copy_from_slice(&[0x04, 0xe0, 0x2d, 0xe5]);
    code
}

pub(crate) fn patch_plt0(plt_data: &mut [u8], plt0_off: usize, _plt0_vaddr: u64, got_vaddr: u64) {
    let got1 = (got_vaddr + 4) as u32;
    let got2 = (got_vaddr + 8) as u32;

    // str lr, [sp, #-4]!
    plt_data[plt0_off..plt0_off + 4].copy_from_slice(&[0x04, 0xe0, 0x2d, 0xe5]);
    // ldr r0, [pc, #8]
    plt_data[plt0_off + 4..plt0_off + 8].copy_from_slice(&[0x08, 0x00, 0x9f, 0xe5]);
    // ldr pc, [pc, #8]
    plt_data[plt0_off + 8..plt0_off + 12].copy_from_slice(&[0x08, 0xf0, 0x9f, 0xe5]);

    plt_data[plt0_off + 16..plt0_off + 20].copy_from_slice(&got2.to_le_bytes());
    plt_data[plt0_off + 20..plt0_off + 24].copy_from_slice(&got1.to_le_bytes());
}

pub(crate) fn generate_plt_entry_code(
    _got_idx: u64,
    reloc_idx: u32,
    plt_entry_offset: u64,
) -> Vec<u8> {
    let mut code = vec![0; 32];
    // ldr ip, [pc, #4]
    // add ip, pc, ip
    // ldr pc, [ip]
    // .word offset
    // mov r1, #reloc_idx
    // b PLT0
    code[0..4].copy_from_slice(&[0x04, 0xc0, 0x9f, 0xe5]);
    code[4..8].copy_from_slice(&[0x0c, 0xc0, 0x8f, 0xe0]);
    code[8..12].copy_from_slice(&[0x00, 0xf0, 0x9c, 0xe5]);

    // mov r1, #reloc_idx
    let mov = 0xe3a01000 | (reloc_idx & 0xff);
    code[16..20].copy_from_slice(&mov.to_le_bytes());

    // b PLT0
    let plt0_off = (-(plt_entry_offset as i64 + 20 + 8) / 4) as i32;
    let b = 0xea000000 | (plt0_off as u32 & 0x00ffffff);
    code[20..24].copy_from_slice(&b.to_le_bytes());

    code
}

pub(crate) fn patch_plt_entry(
    plt_data: &mut [u8],
    plt_entry_off: usize,
    plt_entry_vaddr: u64,
    target_got_vaddr: u64,
) {
    // pc is plt_entry_vaddr + 8
    let offset = (target_got_vaddr as i64 - (plt_entry_vaddr + 8) as i64) as u32;
    plt_data[plt_entry_off + 12..plt_entry_off + 16].copy_from_slice(&offset.to_le_bytes());
}

pub(crate) fn generate_helper_code() -> Vec<u8> {
    // bl <offset>
    // bx lr
    let mut code = vec![0; 8];
    code[0..4].copy_from_slice(&[0x00, 0x00, 0x00, 0xeb]);
    code[4..8].copy_from_slice(&[0x1e, 0xff, 0x2f, 0xe1]);
    code
}

pub(crate) fn patch_helper(
    text_data: &mut [u8],
    helper_text_off: usize,
    helper_vaddr: u64,
    target_plt_vaddr: u64,
) {
    let off = (target_plt_vaddr as i64 - (helper_vaddr + 8) as i64) / 4;
    let insn = 0xeb000000 | ((off & 0x00ffffff) as u32);
    text_data[helper_text_off..helper_text_off + 4].copy_from_slice(&insn.to_le_bytes());
}

pub(crate) fn get_ifunc_resolver_code() -> Vec<u8> {
    let mut code = vec![0; 16];
    // ldr r0, [pc, #0]
    code[0..4].copy_from_slice(&[0x00, 0x00, 0x9f, 0xe5]);
    // bx lr
    code[4..8].copy_from_slice(&[0x1e, 0xff, 0x2f, 0xe1]);
    code
}

pub(crate) fn patch_ifunc_resolver(
    text_data: &mut [u8],
    offset: usize,
    _resolver_vaddr: u64,
    target_vaddr: u64,
) {
    let val = target_vaddr as u32;
    text_data[offset + 8..offset + 12].copy_from_slice(&val.to_le_bytes());
}
