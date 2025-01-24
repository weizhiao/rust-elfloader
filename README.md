[![](https://img.shields.io/crates/v/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![](https://img.shields.io/crates/d/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![license](https://img.shields.io/crates/l/elf_loader.svg)](https://crates.io/crates/elf_loader)
# elf_loader

English | [中文](README_zh.md)  

A `lightweight`, `extensible`, and `fast` library for loading and relocating elf files.    

[Documentation](https://docs.rs/elf_loader/)
# Capabilities
### ✨ Works in `no_std` environments ✨
This crate provides an elf loading interface which does not use any std
features, so it can be used in `no_std` environments such as kernels and embedded device.

### ✨ Fast speed ✨
This crate makes full use of some features of rust and can generate code with excellent performance.

### ✨ Very easy to port and has good extensibility ✨
If you want to port this crate, you only need to implement the `Mmap` trait for your platform. And you can use hook functions to implement additional functionality based on this crate.

### ✨ Tiny library with few dependencies ✨
With minimal features, this crate only depends on the `elf`, `cfg-if`, and `bitflags` crates.

# Usage
It implements the general steps for loading ELF files and leaves extension interfaces, allowing users to implement their own customized loaders.

# Example
## mini-loader
This repository provides an example of a [mini-loader](https://github.com/weizhiao/elf_loader/tree/main/mini-loader) implemented using `elf_loader`. The miniloader can load PIE files and currently only supports `x86_64` .

Load `ls`:

```shell
$ cargo build --release -p mini-loader --target=x86_64-unknown-none
$ ./mini-loader /bin/ls
``` 
It should be noted that mini-loader must be compiled with the release parameter.
## dlopen-rs
[dlopen-rs](https://crates.io/crates/dlopen-rs) is also implemented based on the `elf_loader` library. It implements the functionality of dlopen, allowing dynamic libraries to be opened at runtime.