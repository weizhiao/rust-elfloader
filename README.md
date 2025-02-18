[![](https://img.shields.io/crates/v/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![](https://img.shields.io/crates/d/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![license](https://img.shields.io/crates/l/elf_loader.svg)](https://crates.io/crates/elf_loader)
# elf_loader

English | [中文](README_zh.md)  

The `elf_loader` crate provides an async loading interface for loading ELF dynamic libraries from both memory and files.

[Documentation](https://docs.rs/elf_loader/)
# Capabilities
### ✨ Works in `no_std` environments ✨
This crate provides an elf loading interface which does not use any std
features, so it can be used in `no_std` environments such as kernels and embedded device.

### ✨ Fast speed ✨
This crate makes full use of some features of rust and can generate code with excellent performance. The `elf_loader` is designed to achieve faster performance than `libloading`, specifically aiming to surpass the speed of the dynamic linker/loader (ld.so).

### ✨ Very easy to port and has good extensibility ✨
If you want to port this crate, you only need to implement the `Mmap` trait for your platform. And you can use hook functions to implement additional functionality based on this crate.

### ✨ Tiny library with few dependencies ✨
With minimal features, this crate only depends on the `elf`, `cfg-if`, and `bitflags` crates.

### ✨ Compile-time checking ✨
Utilize Rust's lifetime mechanism to check at compile time whether the dependent libraries of a dynamic library are deallocated prematurely, and whether the dynamic library to which a symbol belongs has been deallocated.   
For example, there are three dynamic libraries loaded by `elf_loader`: `a`, `b`, and `c`. Library `c` depends on `b`, and `b` depends on `a`. If either `a` or `b` is dropped before `c` is dropped, the program will not pass compilation. (You can try this in the [examples/relocate](https://github.com/weizhiao/elf_loader/blob/main/examples/relocate.rs).)

# Usage
It implements the general steps for loading ELF files and leaves extension interfaces, allowing users to implement their own customized loaders.

# Feature

| Feature      |  Description  |
| --------- | ----------------- |
| fs        |  Enable support for filesystems      						|
| use-libc  |  Use libc as the backend, otherwise directly use linux syscalls		|
| mmap      |  Use the default implementation on platforms with mmap when loading ELF files| 
| version   |  Use the version information of symbols when resolving them.     |

# Example
## Load a simple dynamic library
```rust
use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
use std::{collections::HashMap, ptr::null};

fn main() {
    fn print(s: &str) {
        println!("{}", s);
    }

	// Symbols required by dynamic library liba.so
    let mut map = HashMap::new();
    map.insert("__gmon_start__", null());
    map.insert("__cxa_finalize", null());
    map.insert("_ITM_registerTMCloneTable", null());
    map.insert("_ITM_deregisterTMCloneTable", null());
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
This repository provides an example of a [mini-loader](https://github.com/weizhiao/elf_loader/tree/main/mini-loader) implemented using `elf_loader`. The miniloader can load PIE files and currently only supports `x86_64` .

Load `ls`:

```shell
$ cargo build --release -p mini-loader --target=x86_64-unknown-none
$ ./mini-loader /bin/ls
``` 
It should be noted that mini-loader must be compiled with the release parameter.
## dlopen-rs
[dlopen-rs](https://crates.io/crates/dlopen-rs) is also implemented based on the `elf_loader` library. It implements the functionality of dlopen, allowing dynamic libraries to be opened at runtime. And it has implemented hot reloading.

# TODO
* Support more CPU instruction sets (currently only supports AArch64, Riscv64, X86-64).
* Improve support for DT_FLAGS flag bits.
* Improve comments and documentation.
* Add examples (e.g., an example of loading dynamic libraries using an asynchronous interface).
* Add support for more instruction sets in the example mini-loader.
* Add more performance tests and correctness tests.
...

# Supplement
If you encounter any issues during use, feel free to raise them on GitHub. We warmly welcome everyone to contribute code to help improve the functionality of `elf_loader`. 😊