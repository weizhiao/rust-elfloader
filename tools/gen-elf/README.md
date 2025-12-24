# gen-elf

`gen-elf` is a utility for generating ELF files (Shared Objects and Relocatable Objects) specifically designed for testing ELF loaders. It simplifies the process of creating binaries with specific symbol structures and relocation entries for `elf_loader` verification.

## Features

- **Multi-architecture Support**: Supports x86_64, x86, Aarch64, Riscv64, Riscv32, Arm, and Loongarch64.
- **Dynamic Library Generation**: Generates Shared Objects (.so) with `.dynamic` sections, relocation tables (RELA/REL), and symbol tables.
- **Relocatable Object Generation**: Generates standard relocatable object files (.o).
- **High-level API**: Provides intent-based interfaces like `RelocEntry::jump_slot` and `SymbolDesc::global_func`, abstracting away complex ELF constants.
- **Metadata Export**: Exports relocation addresses and other metadata alongside the ELF data for easy verification in tests.

## Core Interfaces

### `DylibWriter`
Used for generating dynamic libraries.

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
Used for generating relocatable object files.

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
Represents the target architecture and provides methods to retrieve architecture-specific relocation types.

- `Arch::current()`: Returns the architecture of the current host.
- `jump_slot_reloc()`: Returns the `JUMP_SLOT` relocation type for the architecture.

## CLI Tool

`gen-elf` can also be used as a standalone command-line tool:

```bash
# Generate a default x86_64 dynamic library
cargo run -p gen-elf -- --dynamic -o ./out

# Generate for a specific architecture
cargo run -p gen-elf -- --target aarch64 --dynamic -o ./out
```

## Usage in Tests

This tool is particularly useful for integration testing of `elf_loader`. You can dynamically generate an ELF with specific relocation types, load it with your loader, and verify that relocations are applied correctly.

See `tests/gen_elf.rs` for examples.
