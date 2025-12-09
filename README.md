<p align="center">
    <img src="./docs/imgs/logo.jpg">
</p>

[![](https://img.shields.io/crates/v/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![](https://img.shields.io/crates/d/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![license](https://img.shields.io/crates/l/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![elf_loader on docs.rs](https://docs.rs/elf_loader/badge.svg)](https://docs.rs/elf_loader)
[![Rust](https://img.shields.io/badge/rust-1.88.0%2B-blue.svg?maxAge=3600)](https://github.com/weizhiao/elf_loader)
[![Build Status](https://github.com/weizhiao/elf_loader/actions/workflows/rust.yml/badge.svg)](https://github.com/weizhiao/elf_loader/actions)

# elf loader

English | [中文](README_zh.md)  

⚡ **High-performance, cross-platform, no-std compatible ELF file loader** ⚡

`elf_loader` can load various forms of ELF files from either memory or storage, and provides efficient runtime linking, including both static and dynamic linking. Whether you are developing an OS kernel, an embedded system, a JIT compiler, or an application that requires dynamic loading of ELF libraries, `elf_loader` delivers exceptional performance and flexibility.

[Documentation](https://docs.rs/elf_loader/) | [Examples](https://github.com/weizhiao/rust-elfloader/tree/main/examples)

---

## 🎯 Core Use Cases

- **Operating System Development** - As an ELF file loader in kernel
- **Dynamic Linker Implementation** - Building a Rust version of the dynamic linker
- **Embedded Systems** - Loading ELF dynamic libraries on resource-constrained devices
- **JIT Compilation Systems** - As a low-level linker for Just-In-Time compilers
- **Cross-platform Development** - Loading ELF dynamic libraries on Windows (see [windows-elf-loader](https://github.com/weizhiao/rust-elfloader/tree/main/crates/windows-elf-loader))

---

## ✨ Outstanding Features

### 🚀 Extreme Performance
Drawing on the implementation essence of `musl` and `glibc`'s `ld.so`, combined with Rust's zero-cost abstractions, it delivers near-native performance:

```shell
# Performance benchmark comparison
elf_loader:new   36.478 µs  
libloading:new   47.065 µs

elf_loader:get   10.477 ns 
libloading:get   93.369 ns
```

### 📦 Ultra Lightweight
The core implementation is extremely compact. The [mini-loader](https://github.com/weizhiao/rust-elfloader/tree/main/crates/mini-loader) built on `elf_loader` compiles to only **34KB**!

### 🔧 No-std Compatible
Fully supports `no_std` environments without enforcing `libc` or OS dependencies, seamlessly usable in kernels and embedded devices.

### 🛡️ Compile-time Safety
Utilizing Rust's lifetime mechanism to check ELF dependency relationships at compile-time, preventing dangling pointers and use-after-free errors:

```rust
// Compilation will fail if dependent libraries are dropped prematurely!
let liba = load_dylib!("liba.so")?;
let libb = load_dylib!("libb.so")?; // Depends on liba
// Dropping liba before libb will cause a compile error
```

### 🔄 Advanced Features Support
- **Lazy Binding** - Symbols resolved on first call, improving startup performance
- **RELR Relocation** - Supports modern relative relocation format, reducing memory footprint
- **Highly Extensible** - Easily port to new platforms through the trait system

---

## 🏗️ Architecture Design

### Easy to Port
Just implement the `Mmap` and `ElfObject` traits for your platform to complete the port. Refer to our [default implementation](https://github.com/weizhiao/rust-elfloader/tree/main/src/os) for quick start.

### Hook Function Extension
Extend functionality through hook functions to implement custom loading logic. See [dlopen-rs hook example](https://github.com/weizhiao/rust-dlopen/blob/main/src/loader.rs).

---

## 📋 Platform Support

| Architecture | Dynamic Linking | Lazy Binding | Static Linking | Test Coverage |
| ------------ | --------------- | ------------ | -------------- | ------------- |
| x86_64       | ✅               | ✅            | ✅              | CI            |
| AArch64      | ✅               | ✅            | TODO           | CI            |
| RISC-V 64/32 | ✅               | ✅            | TODO           | CI/Manual     |
| LoongArch64  | ✅               | ✅            | TODO           | CI            |
| x86          | ✅               | ✅            | TODO           | CI            |
| ARM          | ✅               | ✅            | TODO           | CI            |

---

## 🚀 Quick Start

### Add Dependency
```toml
[dependencies]
elf_loader = "0.13"
```

### Basic Usage
```rust
use elf_loader::load_dylib;
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Provide symbols required by the dynamic library
    let mut symbols = HashMap::new();
    symbols.insert("print", print as *const ());
    
    let pre_find = |name: &str| -> Option<*const ()> {
        symbols.get(name).copied()
    };

    // Load and relocate dynamic library
    let lib = load_dylib!("target/libexample.so")?
        .easy_relocate([].iter(), &pre_find)?;
    
    // Call function in the library
    let func = unsafe { lib.get::<fn() -> i32>("example_function")? };
    println!("Result: {}", func());
    
    Ok(())
}

fn print(s: &str) {
    println!("{}", s);
}
```

---

## ⚙️ Feature Flags

| Feature           | Description                                                   |
| ----------------- | ------------------------------------------------------------- |
| `use-syscall`     | Use Linux system calls as backend                             |
| `version`         | Use version information when resolving symbols                |
| `log`             | Enable logging output                                         |
| `rel`             | Use REL as relocation type                                    |
| `portable-atomic` | Support targets without native pointer size atomic operations |

**Note**: Disable the `use-syscall` feature in environments without an operating system.

---

## 💡 System Requirements

- **Minimum Rust Version**: 1.88.0+
- **Supported Platforms**: All major architectures (see platform support table)

---

## 🤝 Contribution and Support

We warmly welcome community contributions! Whether it's improving core functionality, adding examples, perfecting documentation, or fixing issues, your participation will be highly appreciated.

- **Issue Reporting**: [GitHub Issues](https://github.com/weizhiao/elf_loader/issues)
- **Feature Requests**: Welcome new feature suggestions
- **Code Contribution**: Submit Pull Requests

If this project is helpful to you, please give us a ⭐ to show your support!

## 🎈Contributors

<a href="https://github.com/weizhiao/rust-elfloader/graphs/contributors">
  <img src="https://contributors-img.web.app/image?repo=weizhiao/rust-elfloader" alt="Contributors"/>
</a>

---

**Start using `elf_loader` now to bring efficient ELF loading capabilities to your project!** 🎉