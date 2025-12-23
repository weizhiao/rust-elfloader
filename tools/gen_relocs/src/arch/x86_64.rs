const PADDING: [u8; 4] = [0x0, 0x0, 0x0, 0x0];

pub(crate) fn generate_plt0_code() -> Vec<u8> {
    let mut plt_data = vec![];

    // push qword [rip+offset] ; push GOT[1] (link_map)
    plt_data.extend_from_slice(&[0xff, 0x35]);
    plt_data.extend_from_slice(&PADDING);

    // jmp qword [rip+offset]  ; jump to GOT[2] (_dl_runtime_resolve)
    plt_data.extend_from_slice(&[0xff, 0x25]);
    plt_data.extend_from_slice(&PADDING);

    plt_data.resize(16, 0x90);
    plt_data
}

pub(crate) fn patch_plt0(plt_data: &mut [u8], plt0_off: usize, plt0_vaddr: u64, got_vaddr: u64) {
    // GOT[1]
    let target_got1 = got_vaddr + 8;
    let rip1 = plt0_vaddr + 6;
    let off1 = (target_got1 as i64 - rip1 as i64) as i32;
    plt_data[plt0_off + 2..plt0_off + 6].copy_from_slice(&off1.to_le_bytes());

    // GOT[2]
    let target_got2 = got_vaddr + 16;
    let rip2 = plt0_vaddr + 12;
    let off2 = (target_got2 as i64 - rip2 as i64) as i32;
    plt_data[plt0_off + 8..plt0_off + 12].copy_from_slice(&off2.to_le_bytes());
}

pub(crate) fn generate_plt_entry_code(reloc_idx: u32, plt_entry_offset: u64) -> Vec<u8> {
    let mut plt_data = vec![];
    let plt0_offset = -((plt_entry_offset as i32) + 16);

    // jmp qword [rip+offset] ; jump to GOT entry
    plt_data.extend_from_slice(&[0xff, 0x25]);
    plt_data.extend_from_slice(&PADDING);

    // push index
    plt_data.extend_from_slice(&[0x68]);
    plt_data.extend_from_slice(&reloc_idx.to_le_bytes());

    // jmp PLT[0]
    plt_data.extend_from_slice(&[0xe9]);
    plt_data.extend_from_slice(&plt0_offset.to_le_bytes());

    plt_data.resize(16, 0x90);
    plt_data
}

pub(crate) fn patch_plt_entry(
    plt_data: &mut [u8],
    plt_entry_off: usize,
    plt_entry_vaddr: u64,
    target_got_vaddr: u64,
) {
    let rip = plt_entry_vaddr + 6;
    let off = (target_got_vaddr as i64 - rip as i64) as i32;
    plt_data[plt_entry_off + 2..plt_entry_off + 6].copy_from_slice(&off.to_le_bytes());
}

pub(crate) fn generate_helper_code() -> Vec<u8> {
    // For x86_64: E8 00 00 00 00 (call rel32, 5 bytes)
    // C3 (ret, 1 byte)
    // We use 8 bytes for alignment
    vec![0xE8, 0x00, 0x00, 0x00, 0x00, 0xC3, 0x90, 0x90]
}

pub(crate) fn patch_helper(
    text_data: &mut [u8],
    helper_text_off: usize,
    helper_vaddr: u64,
    target_plt_vaddr: u64,
) {
    // Update call rel32
    // E8 <offset>
    // offset = target - (current_rip)
    // current_rip = helper_vaddr + 5
    let rel_off = (target_plt_vaddr as i64 - (helper_vaddr + 5) as i64) as i32;
    text_data[helper_text_off] = 0xE8;
    text_data[helper_text_off + 1..helper_text_off + 5].copy_from_slice(&rel_off.to_le_bytes());
}

pub(crate) fn get_ifunc_resolver_code() -> Vec<u8> {
    // lea rax, [rip + offset]
    // ret
    let mut code = vec![0x90; 16];
    code[0..3].copy_from_slice(&[0x48, 0x8d, 0x05]);
    code[7] = 0xc3;
    code
}

pub(crate) fn patch_ifunc_resolver(
    text_data: &mut [u8],
    offset: usize,
    resolver_vaddr: u64,
    target_vaddr: u64,
) {
    // rax = rip + offset
    // rip = resolver_vaddr + 7
    let rel_off = (target_vaddr as i64 - (resolver_vaddr + 7) as i64) as i32;
    text_data[offset + 3..offset + 7].copy_from_slice(&rel_off.to_le_bytes());
}
