use anyhow::Result;
use clap::Parser;
use gen_elf::{Arch, DylibWriter, RelocEntry, SymbolDesc};
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

    let relocs = vec![
        RelocEntry::jump_slot(EXTERNAL_FUNC_NAME, arch),
        RelocEntry::jump_slot(EXTERNAL_FUNC_NAME2, arch),
        RelocEntry::glob_dat(EXTERNAL_VAR_NAME, arch),
        RelocEntry::abs(LOCAL_VAR_NAME, arch),
        RelocEntry::copy(COPY_VAR_NAME, arch),
        RelocEntry::dtpoff(EXTERNAL_TLS_NAME, arch),
        RelocEntry::dtpoff(EXTERNAL_TLS_NAME2, arch),
        RelocEntry::relative(arch),
        RelocEntry::irelative(arch),
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

    let writer = gen_elf::ObjectWriter::new(arch);
    let _ = writer.write_file(&out, &symbols, &[])?;
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
