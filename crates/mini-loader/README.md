[![](https://img.shields.io/crates/v/mini-loader.svg)](https://crates.io/crates/mini-loader)
[![](https://img.shields.io/crates/d/mini-loader.svg)](https://crates.io/crates/mini-loader)
[![license](https://img.shields.io/crates/l/mini-loader.svg)](https://crates.io/crates/mini-loader)
[![Rust](https://img.shields.io/badge/rust-1.85.0%2B-blue.svg?maxAge=3600)](https://github.com/weizhiao/elf_loader)

# mini-loader

The mini-loader is capable of loading and executing ELF files, including `Executable file` and `Position-Independent Executable file`

## Note
Support `x86_64`, `riscv64`, `aarch64`.

## Installation
### x86_64
```shell
$ cargo install mini-loader --target x86_64-unknown-none -Zbuild-std=core,alloc,panic_abort -Zbuild-std-features=panic_immediate_abort,optimize_for_size
```
### aarch64
```shell
$ RUSTFLAGS="-C relocation-model=pic -C link-arg=-pie" cargo install mini-loader --target aarch64-unknown-none -Zbuild-std=core,alloc,panic_abort -Zbuild-std-features=panic_immediate_abort,optimize_for_size
```
### riscv64
```shell
$ RUSTFLAGS="-C relocation-model=pic -C link-arg=-pie" cargo install mini-loader --target riscv64gc-unknown-none-elf -Zbuild-std=core,alloc,panic_abort -Zbuild-std-features=panic_immediate_abort,optimize_for_size
```

## Usage
Load and execute `ls`:

```shell
$ mini-loader /bin/ls
``` 