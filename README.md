[![](https://img.shields.io/crates/v/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![](https://img.shields.io/crates/d/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![license](https://img.shields.io/crates/l/elf_loader.svg)](https://crates.io/crates/elf_loader)
# elf_loader
A `lightweight`, `extensible`, and `high-performance` library for loading ELF files.    
English | [中文](README_zh.md)
## Usage
It implements the general steps for loading ELF files and leaves extension interfaces, allowing users to implement their own customized loaders.
## Example
### mini-loader
This repository provides an example of a [mini-loader](https://github.com/weizhiao/elf_loader/tree/main/mini-loader) implemented using `elf_loader`. The miniloader can load PIE files and currently only supports   `x86_64` .

Load `ls`:

```shell
$ cargo build --release -p mini-loader --target=x86_64-unknown-none
$ ./mini-loader /bin/ls
``` 
It should be noted that mini-loader must be compiled with the release parameter.
### dlopen-rs
[dlopen-rs](https://crates.io/crates/dlopen-rs) is also implemented based on the elf_loader library. It implements the functionality of dlopen, allowing dynamic libraries to be opened at runtime.