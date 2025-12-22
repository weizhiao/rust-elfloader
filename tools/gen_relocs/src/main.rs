use anyhow::Result;
use clap::Parser;
use gen_relocs::{gen_static_elf, Arch, DylibWriter, SymbolDesc};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

fn gen_dynamic_elf(out_path: &Path, arch: Arch) -> Result<()> {
    let mut out = out_path.to_path_buf();
    if out.extension().and_then(|s| s.to_str()) != Some("so") {
        out.set_extension("so");
    }

    // Use default configuration for standard ELF generation
    let writer = DylibWriter::new(arch);
    let symbols = vec![
        SymbolDesc::global_func("func_external", &[]),
        SymbolDesc::global_object("var_external", &[]),
        SymbolDesc::global_object("tls_external", &[]),
    ];
    let _ = writer.write_file(&out, &[], &symbols)?;
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
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Target triple (default: all)
    #[arg(short, long, value_enum, default_value_t = Arch::X86_64)]
    target: Arch,
    /// Enable dynamic relocations
    #[arg(short, long, action = clap::ArgAction::SetTrue)]
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
