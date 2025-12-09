<p align="center">
	<img src="./docs/imgs/logo.jpg">
</p>

[![](https://img.shields.io/crates/v/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![](https://img.shields.io/crates/d/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![license](https://img.shields.io/crates/l/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![elf_loader on docs.rs](https://docs.rs/elf_loader/badge.svg)](https://docs.rs/elf_loader)
[![Rust](https://img.shields.io/badge/rust-1.88.0%2B-blue.svg?maxAge=3600)](https://github.com/weizhiao/elf_loader)
[![Build Status](https://github.com/weizhiao/elf_loader/actions/workflows/rust.yml/badge.svg)](https://github.com/weizhiao/elf_loader/actions)

# elf_loader

⚡ **高性能、跨平台、no_std兼容的ELF文件加载器** ⚡

`elf_loader` 能够从内存或文件加载各种形式的ELF文件，并提供运行时高效链接（包括静态链接与动态链接）。无论您是在开发操作系统内核、嵌入式系统、JIT编译器，还是需要动态加载ELF库的应用程序，`elf_loader` 都能提供卓越的性能和灵活性。

[文档](https://docs.rs/elf_loader/) | [示例](https://github.com/weizhiao/rust-elfloader/tree/main/examples)

---

## 🎯 核心应用场景

- **操作系统开发** - 作为内核中的ELF文件加载器
- **动态链接器实现** - 构建Rust版本的动态链接器
- **嵌入式系统** - 在资源受限设备上加载ELF动态库
- **JIT编译系统** - 作为即时编译器的底层链接器
- **跨平台开发** - 在Windows上加载ELF动态库（详见 [windows-elf-loader](https://github.com/weizhiao/rust-elfloader/tree/main/crates/windows-elf-loader)）

---

## ✨ 卓越特性

### 🚀 极致性能
汲取 `musl` 和 `glibc` 中 `ld.so` 的实现精华，结合Rust的零成本抽象，提供接近原生的性能表现：

```shell
# 性能基准测试对比
elf_loader:new   36.478 µs  
libloading:new   47.065 µs

elf_loader:get   10.477 ns 
libloading:get   93.369 ns
```

### 📦 超轻量级
核心实现极其精简，基于 `elf_loader` 构建的 [mini-loader](https://github.com/weizhiao/rust-elfloader/tree/main/crates/mini-loader) 编译后仅 **34KB**！

### 🔧 no_std兼容
完全支持 `no_std` 环境，不强制依赖 `libc` 或操作系统，可在内核和嵌入式设备中无缝使用。

### 🛡️ 编译期安全保障
利用Rust的生命周期机制，在编译期检查ELF依赖关系，防止悬垂指针和use-after-free错误：

```rust
// 如果依赖库在之前被销毁，编译将失败！
let liba = load_dylib!("liba.so")?;
let libb = load_dylib!("libb.so")?; // 依赖 liba
// liba 在 libb 之前被销毁会导致编译错误
```

### 🔄 高级功能支持
- **延迟绑定** - 符号在首次调用时解析，提升启动性能
- **RELR重定位** - 支持现代相对重定位格式，减少内存占用
- **高度可扩展** - 通过trait系统轻松移植到新平台

---

## 🏗️ 架构设计

### 易于移植
只需为您的平台实现 `Mmap` 和 `ElfObject` trait即可完成移植。参考我们的 [默认实现](https://github.com/weizhiao/rust-elfloader/tree/main/src/os) 快速上手。

### 钩子函数扩展
通过hook函数扩展功能，实现自定义加载逻辑，详见 [dlopen-rs hook示例](https://github.com/weizhiao/rust-dlopen/blob/main/src/loader.rs)。

---

## 📋 平台支持

| 指令集       | 动态链接 | 延迟绑定 | 静态链接 | 测试覆盖 |
| ------------ | -------- | -------- | -------- | -------- |
| x86_64       | ✅        | ✅        | ✅        | CI       |
| AArch64      | ✅        | ✅        | TODO     | CI       |
| RISC-V 64/32 | ✅        | ✅        | TODO     | CI/手动  |
| LoongArch64  | ✅        | ✅        | TODO     | CI       |
| x86          | ✅        | ✅        | TODO     | CI       |
| ARM          | ✅        | ✅        | TODO     | CI       |

---

## 🚀 快速开始

### 添加依赖
```toml
[dependencies]
elf_loader = "0.13"
```

### 基本用法
```rust
use elf_loader::load_dylib;
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 提供动态库所需的符号
    let mut symbols = HashMap::new();
    symbols.insert("print", print as *const ());
    
    let pre_find = |name: &str| -> Option<*const ()> {
        symbols.get(name).copied()
    };

    // 加载并重定位动态库
    let lib = load_dylib!("target/libexample.so")?
        .easy_relocate([].iter(), &pre_find)?;
    
    // 调用库中的函数
    let func = unsafe { lib.get::<fn() -> i32>("example_function")? };
    println!("结果: {}", func());
    
    Ok(())
}

fn print(s: &str) {
    println!("{}", s);
}
```

---

## ⚙️ 特性开关

| 特性              | 描述                      |
| ----------------- | ------------------------- |
| `use-syscall`     | 使用Linux系统调用作为后端 |
| `version`         | 在符号解析时使用版本信息  |
| `log`             | 启用日志输出              |
| `rel`             | 使用REL格式的重定位条目   |
| `portable-atomic` | 支持无原生原子操作的目标  |

**注意**: 在无操作系统的环境中请禁用 `use-syscall` 特性。

---

## 💡 系统要求

- **最低Rust版本**: 1.88.0+
- **支持平台**: 所有主要架构（详见平台支持表格）

---

## 🤝 贡献与支持

我们热烈欢迎社区贡献！无论是改进核心功能、增加示例、完善文档还是修复问题，您的参与都将受到高度赞赏。

- **问题反馈**: [GitHub Issues](https://github.com/weizhiao/elf_loader/issues)
- **功能请求**: 欢迎提出新功能建议
- **代码贡献**: 提交Pull Request

如果这个项目对您有帮助，请给我们一个 ⭐ 以表示支持！

## 🎈贡献者

<a href="https://github.com/weizhiao/rust-elfloader/graphs/contributors">
  <img src="https://contributors-img.web.app/image?repo=weizhiao/rust-elfloader" alt="Contributors"/>
</a>

---

**立即开始使用 `elf_loader`，为您的项目带来高效的ELF加载能力！** 🎉