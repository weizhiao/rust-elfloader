pub(crate) fn generate_plt0_code() -> Vec<u8> {
    let code = vec![0; 32];
    // auipc t2, %pcrel_hi(GOT)
    // lw t0, %pcrel_lo(GOT+4)(t2)  ; link_map
    // lw t2, %pcrel_lo(GOT+8)(t2)  ; resolver
    // jr t2
    code
}

pub(crate) fn patch_plt0(
    plt_data: &mut [u8],
    plt0_off: usize,
    plt0_vaddr: u64,
    got_plt_vaddr: u64,
) {
    let pc = plt0_vaddr;
    let target_got1 = got_plt_vaddr + 4;
    let target_got2 = got_plt_vaddr + 8;

    let (hi, lo1) = split_addr(pc, target_got1);
    let (_, lo2) = split_addr(pc, target_got2);

    let auipc = encode_utype(0x17, 7, hi); // t2 is x7
    let lw1 = encode_itype(0x00002003, 5, 7, lo1); // t0 is x5, t2 is x7
    let lw2 = encode_itype(0x00002003, 7, 7, lo2); // t2 is x7
    let jr = encode_itype(0x00000067, 0, 7, 0); // jr t2

    plt_data[plt0_off..plt0_off + 4].copy_from_slice(&auipc.to_le_bytes());
    plt_data[plt0_off + 4..plt0_off + 8].copy_from_slice(&lw1.to_le_bytes());
    plt_data[plt0_off + 8..plt0_off + 12].copy_from_slice(&lw2.to_le_bytes());
    plt_data[plt0_off + 12..plt0_off + 16].copy_from_slice(&jr.to_le_bytes());
}

pub(crate) fn generate_plt_entry_code(reloc_idx: u32, plt_entry_offset: u64) -> Vec<u8> {
    let mut code = vec![0; 32];

    // auipc t2, %pcrel_hi(GOT_ENTRY)
    // lw t2, %pcrel_lo(GOT_ENTRY)(t2)
    // jr t2
    // nop
    // li t1, reloc_idx
    // j PLT0

    let plt0_off = -(plt_entry_offset as i32 + 20);
    let j = encode_jtype(0x6f, 0, plt0_off as u32);

    code[16..20].copy_from_slice(&encode_itype(0x00000013, 6, 0, reloc_idx).to_le_bytes());
    code[20..24].copy_from_slice(&j.to_le_bytes());
    code[12..16].copy_from_slice(&[0x13, 0x00, 0x00, 0x00]); // nop
    code
}

pub(crate) fn patch_plt_entry(
    plt_data: &mut [u8],
    plt_entry_off: usize,
    plt_entry_vaddr: u64,
    target_got_vaddr: u64,
) {
    let pc = plt_entry_vaddr;
    let (hi, lo) = split_addr(pc, target_got_vaddr);

    let auipc = encode_utype(0x17, 7, hi);
    let lw = encode_itype(0x00002003, 7, 7, lo);
    let jr = encode_itype(0x00000067, 0, 7, 0);

    plt_data[plt_entry_off..plt_entry_off + 4].copy_from_slice(&auipc.to_le_bytes());
    plt_data[plt_entry_off + 4..plt_entry_off + 8].copy_from_slice(&lw.to_le_bytes());
    plt_data[plt_entry_off + 8..plt_entry_off + 12].copy_from_slice(&jr.to_le_bytes());
}

fn split_addr(pc: u64, target: u64) -> (u32, u32) {
    let offset = target as i64 - pc as i64;
    let hi = (offset + 0x800) as u32 & 0xfffff000;
    let lo = (offset as u32).wrapping_sub(hi) & 0xfff;
    (hi, lo)
}

fn encode_utype(op: u32, rd: u32, imm: u32) -> u32 {
    op | (rd << 7) | (imm & 0xfffff000)
}

fn encode_itype(op: u32, rd: u32, rs1: u32, imm: u32) -> u32 {
    op | (rd << 7) | (rs1 << 15) | ((imm & 0xfff) << 20)
}

fn encode_stype(op: u32, funct3: u32, rs1: u32, rs2: u32, imm: u32) -> u32 {
    let imm_11_5 = (imm >> 5) & 0x7f;
    let imm_4_0 = imm & 0x1f;
    op | (imm_4_0 << 7) | (funct3 << 12) | (rs1 << 15) | (rs2 << 20) | (imm_11_5 << 25)
}

fn encode_jtype(op: u32, rd: u32, imm: u32) -> u32 {
    let imm20 = (imm >> 20) & 1;
    let imm10_1 = (imm >> 1) & 0x3ff;
    let imm11 = (imm >> 11) & 1;
    let imm19_12 = (imm >> 12) & 0xff;
    op | (rd << 7) | (imm19_12 << 12) | (imm11 << 20) | (imm10_1 << 21) | (imm20 << 31)
}

pub(crate) fn generate_helper_code() -> Vec<u8> {
    // 1. addi sp, sp, -16
    // 2. sw ra, 12(sp)
    // 3. auipc t0, 0
    // 4. jalr ra, t0, 0
    // 5. lw ra, 12(sp)
    // 6. addi sp, sp, 16
    // 7. ret
    let mut code = vec![0; 28];

    let addi_sp_down = encode_itype(0x13, 2, 2, (-16i32) as u32);
    let sw_ra = encode_stype(0x23, 2, 2, 1, 12);
    let auipc = encode_utype(0x17, 5, 0); // t0 is x5
    let jalr = encode_itype(0x67, 1, 5, 0); // jalr ra, t0, 0
    let lw_ra = encode_itype(0x03, 1, 2, 12);
    let addi_sp_up = encode_itype(0x13, 2, 2, 16);
    let ret = 0x00008067u32;

    code[0..4].copy_from_slice(&addi_sp_down.to_le_bytes());
    code[4..8].copy_from_slice(&sw_ra.to_le_bytes());
    code[8..12].copy_from_slice(&auipc.to_le_bytes());
    code[12..16].copy_from_slice(&jalr.to_le_bytes());
    code[16..20].copy_from_slice(&lw_ra.to_le_bytes());
    code[20..24].copy_from_slice(&addi_sp_up.to_le_bytes());
    code[24..28].copy_from_slice(&ret.to_le_bytes());

    code
}

pub(crate) fn patch_helper(
    text_data: &mut [u8],
    helper_text_off: usize,
    helper_vaddr: u64,
    target_plt_vaddr: u64,
) {
    // auipc is at helper_vaddr + 8
    let pc = helper_vaddr + 8;
    let off = target_plt_vaddr as i64 - pc as i64;
    let hi = (off + 0x800) as u32 & 0xfffff000;
    let lo = (off as u32).wrapping_sub(hi) & 0xfff;

    let auipc = encode_utype(0x17, 5, hi);
    let jalr = encode_itype(0x67, 1, 5, lo);

    text_data[helper_text_off + 8..helper_text_off + 12].copy_from_slice(&auipc.to_le_bytes());
    text_data[helper_text_off + 12..helper_text_off + 16].copy_from_slice(&jalr.to_le_bytes());
}

pub(crate) fn get_ifunc_resolver_code() -> Vec<u8> {
    let mut code = vec![0; 20];
    // auipc a0, 0
    // lw t0, 16(a0)
    // add a0, a0, t0
    // ret
    // <target_offset (4 bytes)>
    code[0..4].copy_from_slice(&[0x17, 0x05, 0x00, 0x00]);
    code[4..8].copy_from_slice(&[0x83, 0x22, 0x05, 0x01]); // lw t0, 16(a0)
    code[8..12].copy_from_slice(&[0x33, 0x05, 0x55, 0x00]); // add a0, a0, t0
    code[12..16].copy_from_slice(&[0x67, 0x80, 0x00, 0x00]); // ret
    code
}

pub(crate) fn patch_ifunc_resolver(
    text_data: &mut [u8],
    offset: usize,
    resolver_vaddr: u64,
    target_vaddr: u64,
) {
    let rel_off = (target_vaddr as i64 - resolver_vaddr as i64) as i32;
    text_data[offset + 16..offset + 20].copy_from_slice(&rel_off.to_le_bytes());
}
