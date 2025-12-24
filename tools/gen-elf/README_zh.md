# gen-elf

`gen-elf` 是一个用于生成测试用 ELF 文件（动态库和可重定位对象文件）的辅助工具。它旨在简化在测试 `elf_loader` 时创建具有特定符号和重定位结构的二进制文件的过程。

## 功能特性

- **多架构支持**：支持 x86_64, x86, Aarch64, Riscv64, Riscv32, Arm, Loongarch64。
- **动态库生成**：支持生成带有 `.dynamic` 段、重定位表（RELA/REL）和符号表的共享对象（.so）。
- **可重定位文件生成**：支持生成标准的 `.o` 文件。
- **高层级 API**：提供意图导向的接口，如 `RelocEntry::jump_slot` 和 `SymbolDesc::global_func`，无需手动处理复杂的 ELF 常量。
- **元数据导出**：生成 ELF 的同时导出重定位地址等元数据，方便在测试中进行验证。

## 核心接口

### `DylibWriter`
用于生成动态库。

```rust
use gen_elf::{Arch, DylibWriter, RelocEntry, SymbolDesc};

let arch = Arch::X86_64;
let writer = DylibWriter::new(arch);

let relocs = vec![
    RelocEntry::jump_slot("external_func", arch),
];

let symbols = vec![
    SymbolDesc::global_object("my_var", &[1, 2, 3, 4]),
    SymbolDesc::undefined_func("external_func"),
];

let output = writer.write_file("libtest.so", &relocs, &symbols)?;
println!("Generated ELF at base address: {:#x}", output.base_addr);
```

### `ObjectWriter`
用于生成可重定位对象文件。

```rust
use gen_elf::{Arch, ObjectWriter, SymbolDesc};

let arch = Arch::X86_64;
let writer = ObjectWriter::new(arch);

let symbols = vec![
    SymbolDesc::global_func("my_func", &[0x90, 0xc3]), // nop; ret
];

writer.write_file("test.o", &symbols, &[])?;
```

### `Arch`
表示目标架构，提供架构相关的重定位类型获取方法。

- `Arch::current()`: 获取当前主机的架构。
- `jump_slot_reloc()`: 获取该架构的 `JUMP_SLOT` 重定位类型。

## 命令行工具

`gen-elf` 也可以作为一个独立的命令行工具使用：

```bash
# 生成默认的 x86_64 动态库
cargo run -p gen-elf -- --dynamic -o ./out

# 为指定架构生成
cargo run -p gen-elf -- --target aarch64 --dynamic -o ./out
```

## 在测试中使用

该工具特别适合用于 `elf_loader` 的集成测试。你可以动态生成一个具有特定重定位类型的 ELF，然后使用你的加载器加载它，并验证重定位是否正确应用。
