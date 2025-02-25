[![](https://img.shields.io/crates/v/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![](https://img.shields.io/crates/d/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![license](https://img.shields.io/crates/l/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![elf_loader on docs.rs](https://docs.rs/elf_loader/badge.svg)](https://docs.rs/elf_loader)
[![Rust](https://img.shields.io/badge/rust-1.85.0%2B-blue.svg?maxAge=3600)](https://github.com/weizhiao/elf_loader)
[![Build Status](https://github.com/weizhiao/elf_loader/actions/workflows/rust.yml/badge.svg)](https://github.com/cole14/rust-elf/actions)

# elf_loader

English | [中文](README_zh.md)  

`elf_loader` can load various forms of ELF files from memory or files, including `Executable file`, `Shared object file`, and `Position-Independent Executable file`.

[Documentation](https://docs.rs/elf_loader/)

# Usage
`elf_loader` can load various ELF files and provides interfaces for extended functionality. It can be used in the following areas:
* Use it as an ELF file loader in operating system kernels
* Use it to implement a Rust version of the dynamic linker
* Use it to load ELF dynamic libraries on embedded devices

# Capabilities
### ✨ Works in `no_std` environments ✨
`elf_loader` does not depend on Rust `std`, nor does it enforce `libc` and OS dependencies, so it can be used in `no_std` environments such as kernel and embedded devices.

### ✨ Fast speed ✨
This library draws on the strengths of `musl` and `glibc`'s `ld.so` implementation and fully utilizes some features of Rust (such as static dispatch), allowing it to generate `high-performance` code. [dlopen-rs](https://crates.io/crates/dlopen-rs) based on `elf_loader` has better performance than `libloading`.

### ✨ Very easy to port and has good extensibility ✨
If you want to port `elf_loader`, you only need to implement the `Mmap` and `ElfObject` traits for your platform. When implementing the `Mmap` trait, you can refer to the default implementation provided by `elf_loader`: [mmap](https://github.com/weizhiao/elf_loader/tree/main/src/mmap). In addition, you can use the `hook` functions provided by this library to extend the functionality of `elf_loader` to implement any other features you want. When using the `hook` functions, you can refer to: [hook](https://github.com/weizhiao/dlopen-rs/blob/main/src/loader/mod.rs) in `dlopen-rs`.

### ✨ Provides asynchronous interfaces ✨
`elf_loader` provides asynchronous interfaces for loading ELF files, which can achieve higher performance in scenarios where ELF files are loaded concurrently.   
However, you need to implement the `Mmap` and `ElfObjectAsync` traits according to your application scenario. For example, instead of using `mmap` to directly map ELF files, you can use a combination of `mmap` and file reading (`mmap` creates memory space, and then the content of the ELF file is read into the space created by `mmap`) to load ELF files, thus fully utilizing the advantages brought by the asynchronous interface.

### ✨ Compile-time checking ✨
Utilize Rust's lifetime mechanism to check at compile time whether the dependent libraries of a dynamic library are deallocated prematurely.   
For example, there are three dynamic libraries loaded by `elf_loader`: `a`, `b`, and `c`. Library `c` depends on `b`, and `b` depends on `a`. If either `a` or `b` is dropped before `c` is dropped, the program will not pass compilation. (You can try this in the [examples/relocate](https://github.com/weizhiao/elf_loader/blob/main/examples/relocate.rs).)

# Feature

| Feature      |  Description  |
| --------- | ----------------- |
| fs        |  Enable support for filesystems      						|
| use-libc  |  This feature works when the `fs` or `mmap `feature is enabled. If `use-libc` is enabled, `elf_loader` will use `libc` as the backend, otherwise it will just use `linux syscalls`		|
| mmap      |  Use the default implementation on platforms with mmap when loading ELF files| 
| version   |  Use the version information of symbols when resolving them.     |
| log   |  Enable logging     |

Disable the `fs`,`use-libc` and `mmap` features if you don't have an operating system.

# Example
## Load a simple dynamic library
```rust
use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
use std::collections::HashMap;

fn main() {
    fn print(s: &str) {
        println!("{}", s);
    }

	// Symbols required by dynamic library liba.so
    let mut map = HashMap::new();
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
	// Load dynamic library liba.so 
	let loader = Loader::<MmapImpl>::new();
    let liba = loader
        .easy_load_dylib(ElfFile::from_path("target/liba.so").unwrap())
        .unwrap();
	// Relocate symbols in liba.so
    let a = liba.easy_relocate([].iter(), &pre_find).unwrap();
	// Call function a in liba.so
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    f();
}
```

## mini-loader
[mini-loader](https://github.com/weizhiao/elf_loader/tree/main/mini-loader) is implemented based on the `elf_loader` library. mini-loader can load and execute elf files, and currently only supports `x86_64`.

# TODO
* Support more CPU instruction sets (currently only supports `AArch64`, `Riscv64`, `X86-64`).
* Improve support for `DT_FLAGS` flag bits.
* Improve comments and documentation.
* Add support for more instruction sets in the example mini-loader.
* Add more performance tests and correctness tests.
* Further optimize performance using portable simd.  
...

# Minimum Supported Rust Version
Rust 1.85 or higher.

# Supplement
If you encounter any issues while using it, you can raise an issue on GitHub. Additionally, we warmly welcome any friends interested in the `elf_loader` to contribute code (improving `elf_loader` itself, adding examples, and fixing issues in the documentation are all welcome). If you find `elf_loader` helpful, feel free to give it a star.
😊