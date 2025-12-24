// 辅助常量：寄存器编号
const REG_T0: u32 = 5;
const REG_T1: u32 = 6;
const REG_T2: u32 = 7;
const REG_T3: u32 = 28;
const REG_RA: u32 = 1;
const REG_SP: u32 = 2;

pub(crate) fn generate_plt0_code() -> Vec<u8> {
    // 现代 glibc PLT0 (32 bytes)
    // 59e0: auipc t2, 0x?
    // 59e4: sub   t1, t1, t3
    // 59e8: ld    t3, ?(t2)
    // 59ec: addi  t1, t1, ?
    // 59f0: addi  t0, t2, ?
    // 59f4: srli  t1, t1, 0x1    <-- 关键修正
    // 59f8: ld    t0, 8(t0)
    // 59fc: jr    t3

    let mut code = vec![0; 32];

    // 1. auipc t2, 0 (Placeholder)
    let auipc = encode_utype(0x17, REG_T2, 0);

    // 2. sub t1, t1, t3
    // opcode=0x33, funct3=0, funct7=0x20
    let sub = encode_rtype(0x33, REG_T1, REG_T1, REG_T3, 0, 0x20);

    // 3. ld t3, 0(t2) (Placeholder)
    // opcode=0x03, funct3=3 (LD) -> 0x3003
    let ld_resolver = encode_itype(0x3003, REG_T3, REG_T2, 0);

    // 4. addi t1, t1, -44
    // opcode=0x13, funct3=0 (ADDI) -> 0x13
    let addi_adj = encode_itype(0x13, REG_T1, REG_T1, (-44i32) as u32);

    // 5. addi t0, t2, 0 (Placeholder)
    let addi_got = encode_itype(0x13, REG_T0, REG_T2, 0);

    // 6. srli t1, t1, 1  (FIXED)
    // 0x00135313 corresponds to:
    // opcode: 0010011 (0x13)
    // rd:     00110 (x6/t1)
    // funct3: 101 (SRLI)
    // rs1:    00110 (x6/t1)
    // shamt:  00001 (1)
    // funct7: 0000000
    let srli = 0x00135313u32;

    // 7. ld t0, 8(t0)
    let ld_linkmap = encode_itype(0x3003, REG_T0, REG_T0, 8);

    // 8. jr t3
    // jalr x0, 0(t3) -> opcode=0x67, funct3=0
    let jr = encode_itype(0x67, 0, REG_T3, 0);

    code[0..4].copy_from_slice(&auipc.to_le_bytes());
    code[4..8].copy_from_slice(&sub.to_le_bytes());
    code[8..12].copy_from_slice(&ld_resolver.to_le_bytes());
    code[12..16].copy_from_slice(&addi_adj.to_le_bytes());
    code[16..20].copy_from_slice(&addi_got.to_le_bytes());
    code[20..24].copy_from_slice(&srli.to_le_bytes());
    code[24..28].copy_from_slice(&ld_linkmap.to_le_bytes());
    code[28..32].copy_from_slice(&jr.to_le_bytes());

    code
}

pub(crate) fn patch_plt0(
    plt_data: &mut [u8],
    plt0_off: usize,
    plt0_vaddr: u64,
    got_plt_vaddr: u64,
) {
    let pc = plt0_vaddr;
    // Calculate offset to GOT[0]
    let (hi, lo) = split_addr(pc, got_plt_vaddr);

    // 1. auipc t2, hi
    let auipc = encode_utype(0x17, REG_T2, hi);
    // 3. ld t3, lo(t2)
    let ld_resolver = encode_itype(0x00003003, REG_T3, REG_T2, lo);
    // 5. addi t0, t2, lo
    let addi_got = encode_itype(0x13, REG_T0, REG_T2, lo);

    plt_data[plt0_off..plt0_off + 4].copy_from_slice(&auipc.to_le_bytes());
    // skip sub (4..8)
    plt_data[plt0_off + 8..plt0_off + 12].copy_from_slice(&ld_resolver.to_le_bytes());
    // skip addi_adj (12..16)
    plt_data[plt0_off + 16..plt0_off + 20].copy_from_slice(&addi_got.to_le_bytes());
}

pub(crate) fn generate_plt_entry_code() -> Vec<u8> {
    // New PLT Entry (16 bytes)
    // auipc t3, %pcrel_hi(GOT_ENTRY)
    // ld t3, %pcrel_lo(GOT_ENTRY)(t3)
    // jalr t1, t3, 0
    // nop
    let mut code = vec![0; 16];

    // Instructions will be patched later, just filling placeholders or NOPs
    // Note: jalr instruction is constant except for registers
    // jalr t1, t3, 0
    let jalr = encode_itype(0x67, REG_T1, REG_T3, 0);
    // nop
    let nop = 0x00000013u32;

    // We can pre-fill the last two instructions as they don't depend on layout
    code[8..12].copy_from_slice(&jalr.to_le_bytes());
    code[12..16].copy_from_slice(&nop.to_le_bytes());

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

    // auipc t3, hi
    let auipc = encode_utype(0x17, REG_T3, hi);
    // ld t3, lo(t3)
    let ld = encode_itype(0x00003003, REG_T3, REG_T3, lo);

    plt_data[plt_entry_off..plt_entry_off + 4].copy_from_slice(&auipc.to_le_bytes());
    plt_data[plt_entry_off + 4..plt_entry_off + 8].copy_from_slice(&ld.to_le_bytes());
}

// 辅助函数保持不变
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

// 新增 R-Type 编码器用于 sub 指令
fn encode_rtype(op: u32, rd: u32, rs1: u32, rs2: u32, funct3: u32, funct7: u32) -> u32 {
    op | (rd << 7) | (funct3 << 12) | (rs1 << 15) | (rs2 << 20) | (funct7 << 25)
}

pub(crate) fn generate_helper_code() -> Vec<u8> {
    // 总共 7 条指令，28 字节
    let mut code = vec![0; 28];

    // 1. addi sp, sp, -16
    // opcode=0x13, rd=2, rs1=2, imm=-16
    let addi_sp_down = encode_itype(0x13, REG_SP, REG_SP, (-16i32) as u32);

    // 2. sd ra, 8(sp)
    // S-Type: opcode=0x23, funct3=3, rs1=sp(2), rs2=ra(1), imm=8
    // hex: 0x00113423
    let sd_ra = 0x00113423u32;

    // 3. auipc t0, 0 (Placeholder)
    let auipc = encode_utype(0x17, REG_T0, 0);

    // 4. jalr ra, t0, 0 (Placeholder)
    let jalr = encode_itype(0x67, REG_RA, REG_T0, 0);

    // 5. ld ra, 8(sp)
    // I-Type: opcode=0x03, funct3=3, rd=ra(1), rs1=sp(2), imm=8
    // hex: 0x00813083
    let ld_ra = 0x00813083u32;

    // 6. addi sp, sp, 16
    let addi_sp_up = encode_itype(0x13, REG_SP, REG_SP, 16);

    // 7. ret (jr ra) => jalr x0, 0(ra)
    let ret = 0x00008067u32;

    // 填入 Buffer
    code[0..4].copy_from_slice(&addi_sp_down.to_le_bytes());
    code[4..8].copy_from_slice(&sd_ra.to_le_bytes());
    code[8..12].copy_from_slice(&auipc.to_le_bytes()); // Patch target
    code[12..16].copy_from_slice(&jalr.to_le_bytes()); // Patch target
    code[16..20].copy_from_slice(&ld_ra.to_le_bytes());
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
    // 注意：auipc 指令现在位于 helper 的第 8 个字节处 (索引 2)
    // 所以 auipc 的 PC 是 helper_vaddr + 8
    let pc = helper_vaddr + 8;

    let off = target_plt_vaddr as i64 - pc as i64;
    let hi = (off + 0x800) as u32 & 0xfffff000;
    let lo = (off as u32).wrapping_sub(hi) & 0xfff;

    let auipc = encode_utype(0x17, REG_T0, hi);
    let jalr = encode_itype(0x67, REG_RA, REG_T0, lo);

    // 这里的偏移量也要相应调整：+8 和 +12
    text_data[helper_text_off + 8..helper_text_off + 12].copy_from_slice(&auipc.to_le_bytes());
    text_data[helper_text_off + 12..helper_text_off + 16].copy_from_slice(&jalr.to_le_bytes());
}

pub(crate) fn get_ifunc_resolver_code() -> Vec<u8> {
    let mut code = vec![0; 24];
    // auipc a0, 0
    // ld t0, 16(a0)
    // add a0, a0, t0
    // ret
    // <target_offset (8 bytes)>
    code[0..4].copy_from_slice(&[0x17, 0x05, 0x00, 0x00]);
    code[4..8].copy_from_slice(&[0x83, 0x32, 0x05, 0x01]); // ld t0, 16(a0)
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
    let rel_off = target_vaddr as i64 - resolver_vaddr as i64;
    text_data[offset + 16..offset + 24].copy_from_slice(&rel_off.to_le_bytes());
}
