use anyhow::Result;
use clap::Parser;
use gen_relocs::{Arch, DylibWriter, RelocEntry, SymbolDesc, gen_static_elf};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

const EXTERNAL_FUNC_NAME: &str = "external_func";
const EXTERNAL_FUNC_NAME2: &str = "external_func2";
const EXTERNAL_VAR_NAME: &str = "external_var";
const EXTERNAL_TLS_NAME: &str = "external_tls";
const EXTERNAL_TLS_NAME2: &str = "external_tls2";
const COPY_VAR_NAME: &str = "copy_var";
const LOCAL_VAR_NAME: &str = "local_var";

fn gen_dynamic_elf(out_path: &Path, arch: Arch) -> Result<()> {
    let mut out = out_path.to_path_buf();
    if out.extension().and_then(|s| s.to_str()) != Some("so") {
        out.set_extension("so");
    }

    let (rel_jump_slot, rel_got, rel_symbolic, rel_copy, rel_dtpoff, rel_relative, rel_irelative) =
        match arch {
            Arch::X86_64 => (
                object::elf::R_X86_64_JUMP_SLOT,
                object::elf::R_X86_64_GLOB_DAT,
                object::elf::R_X86_64_64,
                object::elf::R_X86_64_COPY,
                object::elf::R_X86_64_DTPOFF64,
                object::elf::R_X86_64_RELATIVE,
                object::elf::R_X86_64_IRELATIVE,
            ),
            Arch::Aarch64 => (
                object::elf::R_AARCH64_JUMP_SLOT,
                object::elf::R_AARCH64_GLOB_DAT,
                object::elf::R_AARCH64_ABS64,
                object::elf::R_AARCH64_COPY,
                object::elf::R_AARCH64_TLS_DTPMOD, // Simplified
                object::elf::R_AARCH64_RELATIVE,
                object::elf::R_AARCH64_IRELATIVE,
            ),
            Arch::X86 => (
                object::elf::R_386_JMP_SLOT,
                object::elf::R_386_GLOB_DAT,
                object::elf::R_386_32,
                object::elf::R_386_COPY,
                object::elf::R_386_TLS_DTPMOD32,
                object::elf::R_386_RELATIVE,
                object::elf::R_386_IRELATIVE,
            ),
            Arch::Riscv64 => (
                object::elf::R_RISCV_JUMP_SLOT,
                0,
                object::elf::R_RISCV_64,
                object::elf::R_RISCV_COPY,
                object::elf::R_RISCV_TLS_DTPMOD64,
                object::elf::R_RISCV_RELATIVE,
                object::elf::R_RISCV_IRELATIVE,
            ),
            _ => (0, 0, 0, 0, 0, 0, 0),
        };

    let relocs = vec![
        RelocEntry::with_name(EXTERNAL_FUNC_NAME, rel_jump_slot),
        RelocEntry::with_name(EXTERNAL_FUNC_NAME2, rel_jump_slot),
        RelocEntry::with_name(EXTERNAL_VAR_NAME, rel_got),
        RelocEntry::with_name(LOCAL_VAR_NAME, rel_symbolic),
        RelocEntry::with_name(COPY_VAR_NAME, rel_copy),
        RelocEntry::with_name(EXTERNAL_TLS_NAME, rel_dtpoff),
        RelocEntry::with_name(EXTERNAL_TLS_NAME2, rel_dtpoff),
        RelocEntry::new(rel_relative),
        RelocEntry::new(rel_irelative),
    ];

    // Use default configuration for standard ELF generation
    let writer = DylibWriter::new(arch);
    let symbols = vec![
        SymbolDesc::global_object(LOCAL_VAR_NAME, &[0u8; 8]),
        SymbolDesc::undefined_func(EXTERNAL_FUNC_NAME),
        SymbolDesc::undefined_func(EXTERNAL_FUNC_NAME2),
        SymbolDesc::undefined_object(EXTERNAL_VAR_NAME),
        SymbolDesc::undefined_tls(EXTERNAL_TLS_NAME),
        SymbolDesc::undefined_tls(EXTERNAL_TLS_NAME2),
        SymbolDesc::undefined_object(COPY_VAR_NAME).with_size(8),
    ];
    let _ = writer.write_file(&out, &relocs, &symbols)?;
    println!("Wrote {}", out.display());
    Ok(())
}

fn gen_relocatable_elf(out_path: &Path, arch: Arch) -> Result<()> {
    let mut out = out_path.to_path_buf();
    if out.extension().and_then(|s| s.to_str()) != Some("o") {
        out.set_extension("o");
    }

    let symbols = vec![
        SymbolDesc::global_func("func_external", &[]),
        SymbolDesc::global_object("var_external", &[]),
        SymbolDesc::global_object("tls_external", &[]),
    ];

    let elf_data = gen_static_elf(arch, &symbols, &[])?;
    let mut f = File::create(&out)?;
    f.write_all(&elf_data.data)?;
    println!("Wrote {}", out.display());
    Ok(())
}

pub fn gen_elf(out_path: &Path, arch: Arch, dynamic: bool) -> Result<()> {
    if dynamic {
        gen_dynamic_elf(out_path, arch)?;
    } else {
        gen_relocatable_elf(out_path, arch)?;
    }
    Ok(())
}

#[derive(Parser)]
#[command(name = "gen_relocs")]
struct Args {
    /// Output directory for generated artifacts
    #[arg(short)]
    output: Option<PathBuf>,
    /// Target triple (default: all)
    #[arg(short, long, value_enum, default_value_t = Arch::X86_64)]
    target: Arch,
    /// Enable dynamic relocations
    #[arg(short,  action = clap::ArgAction::SetTrue)]
    dynamic: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let output = match args.output {
        Some(p) => p,
        None => {
            let mut cwd = std::env::current_dir()?;
            cwd.push("out");
            cwd
        }
    };

    println!(
        "out: {}\ntarget: {:?}\ndynamic: {}",
        output.display(),
        args.target,
        args.dynamic
    );

    gen_elf(&output, args.target, args.dynamic)?;

    Ok(())
}
