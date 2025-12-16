//! Example demonstrating custom ELF writer configuration
//!
//! This example shows how to use ElfWriterConfig to customize ELF generation parameters.

use gen_relocs::writer::{ElfWriter, ElfWriterConfig, SymbolDesc};
use gen_relocs::Arch;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    // Example 1: Using default configuration
    println!("Example 1: Default configuration");
    let writer = ElfWriter::new(Arch::X86_64);
    println!("  Created writer with default config (base: 0x400000, page_size: 0x1000)");

    // Example 2: Custom configuration
    println!("\nExample 2: Custom configuration");
    let custom_config = ElfWriterConfig::default()
        .with_base_addr(0x7f000000)  // Load at higher address
        .with_page_size(0x2000)       // Larger page size
        .with_initial_data_size(0x5000); // Larger data segment

    let writer = ElfWriter::with_config(Arch::Aarch64, custom_config);
    println!("  Created writer with custom config");
    println!("    - Base address: 0x7f000000");
    println!("    - Page size: 0x2000");
    println!("    - Initial data size: 0x5000");

    // Example 3: Symbol definitions
    println!("\nExample 3: Symbol definitions");
    let symbols = vec![
        SymbolDesc::global_func("my_function"),
        SymbolDesc::global_object("my_variable"),
        SymbolDesc::local_var("local_symbol", 0x100),
    ];
    println!("  Defined {} symbols", symbols.len());
    for sym in &symbols {
        println!("    - {}", sym.name);
    }

    Ok(())
}
