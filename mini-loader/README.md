[![](https://img.shields.io/crates/v/mini-loader.svg)](https://crates.io/crates/mini-loader)
[![](https://img.shields.io/crates/d/mini-loader.svg)](https://crates.io/crates/mini-loader)
[![license](https://img.shields.io/crates/l/mini-loader.svg)](https://crates.io/crates/mini-loader)
[![Rust](https://img.shields.io/badge/rust-1.85.0%2B-blue.svg?maxAge=3600)](https://github.com/weizhiao/elf_loader)

# mini-loader

The mini-loader is capable of loading and executing ELF files, including `Executable file` and `Position-Independent Executable file`

## Note
Currently only support `x86_64` .

## Installation
```shell
$ cargo install mini-loader --target x86_64-unknown-none -Zbuild-std=core,alloc,panic_abort -Zbuild-std-features=panic_immediate_abort,optimize_for_size
```

## Usage
Load and execute `ls`:

```shell
$ mini-loader /bin/ls
``` 