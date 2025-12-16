# gen_relocs - ELF Generator with Relocation Support

A flexible ELF binary generator for creating test fixtures with relocations, supporting multiple architectures and customizable generation parameters.

## Features

- **Multi-Architecture Support**: x86_64, ARM, ARM64, RISC-V (32-bit and 64-bit)
- **Configurable Generation**: Customize base address, page size, segment sizes
- **Relocation Metadata**: Collect detailed information about each relocation for verification
- **Dynamic Linking**: Full support for dynamic symbol tables, relocation tables, and dynamic sections
- **Builder Pattern**: Ergonomic API using Builder pattern for configuration

## Architecture

### Core Components

- **ElfWriterConfig**: Configuration struct for customizing ELF generation parameters
  - `base_addr`: Virtual address where ELF will be loaded (default: 0x400000)
  - `page_size`: Alignment for program headers (default: 0x1000)
  - `initial_data_size`: Size of .data segment (default: 0x3000)
  - `section_align`: ELF section alignment (default: 8)

- **ElfWriter**: Main writer handling ELF generation
  - `new(arch)`: Create with default configuration
  - `with_config(arch, config)`: Create with custom configuration
  - `write_elf(relocs, symbols)`: Generate ELF and return output with metadata

- **SymbolDesc**: Symbol definition with flexible constructors
  - `global_func(name)`: External function symbol
  - `global_object(name)`: External data symbol
  - `local_var(name, offset)`: Locally defined symbol

- **ElfWriteOutput**: Complete generation output
  - `data`: Raw ELF bytes
  - `base_addr`: Base address used
  - `data_vaddr`: Data segment virtual address
  - `text_vaddr`: Text segment virtual address
  - `relocations`: Metadata for each relocation

## Usage

### Basic Usage with Default Configuration

```rust
use gen_relocs::writer::{ElfWriter, SymbolDesc};
use gen_relocs::Arch;

let writer = ElfWriter::new(Arch::X86_64);
let symbols = vec![
    SymbolDesc::global_func("malloc"),
    SymbolDesc::global_object("errno"),
];
let output = writer.write_elf(&relocs, &symbols)?;
std::fs::write("test.so", &output.data)?;
```

### Custom Configuration

```rust
use gen_relocs::writer::{ElfWriter, ElfWriterConfig};
use gen_relocs::Arch;

let config = ElfWriterConfig::default()
    .with_base_addr(0x7f000000)
    .with_page_size(0x2000);
    
let writer = ElfWriter::with_config(Arch::Aarch64, config);
let output = writer.write_elf(&relocs, &symbols)?;
```

## Module Structure

- `arch/`: Architecture-specific relocation definitions
  - `x86_64.rs`: x86_64 architecture support
  - `aarch64.rs`: ARM64 architecture support
  - `arm.rs`: ARM (32-bit) support
  - `riscv32.rs`, `riscv64.rs`: RISC-V support

- `writer.rs`: Core ELF writing infrastructure
  - `ElfWriterConfig`: Configuration management
  - `ElfWriter`: Main writer implementation
  - Layout calculation and section generation

- `common.rs`: Shared types and fixtures
  - `RelocEntry`: Relocation entry definition
  - `FixtureSym`: Symbol fixture enum
  - `RelocFixtures`: Common relocation patterns

- `lib.rs`: Public API and ELF generation functions
  - `gen_dynamic_elf()`: Generate dynamic library
  - `gen_static_elf()`: Generate static object file
  - `get_relocs_dynamic()`: Get architecture-specific relocations

## Design Improvements

### Enhanced Flexibility

The `ElfWriterConfig` struct provides a Builder pattern API allowing:
- Override any generation parameter
- Support for custom memory layouts
- Experimentation with different configurations

### Clearer API

SymbolDesc constructors are now explicit about symbol types:
- `global_func()`: Creates STT_FUNC symbol with STB_GLOBAL binding
- `global_object()`: Creates STT_OBJECT symbol with STB_GLOBAL binding
- `local_var()`: Creates locally-defined symbol with specific section offset

### Better Organization

Code is logically organized with:
- Clear separation between configuration and generation
- Well-documented public interfaces
- Private internal helpers for implementation details

## Examples

See `examples/custom_config.rs` for a complete example of using custom configurations.

## Testing

Run tests to verify ELF generation correctness:

```bash
cargo test --test loading
```

This validates that generated ELF files can be loaded and relocated correctly.
