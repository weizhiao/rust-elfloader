use std::path::PathBuf;

use clap::Parser;
use gen_relocs::Arch;

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

    gen_relocs::gen_elf(&output, args.target, args.dynamic)?;

    Ok(())
}
