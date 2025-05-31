[![](https://img.shields.io/crates/v/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![](https://img.shields.io/crates/d/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![license](https://img.shields.io/crates/l/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![elf_loader on docs.rs](https://docs.rs/elf_loader/badge.svg)](https://docs.rs/elf_loader)
[![Rust](https://img.shields.io/badge/rust-1.85.0%2B-blue.svg?maxAge=3600)](https://github.com/weizhiao/elf_loader)
[![Build Status](https://github.com/weizhiao/elf_loader/actions/workflows/rust.yml/badge.svg)](https://github.com/weizhiao/elf_loader/actions)
# elf_loader
`elf_loader`能够从内存、文件加载并重定位各种形式的elf文件，包括`Executable file`、`Shared object file`和`Position-Independent Executable file`。  

[文档](https://docs.rs/elf_loader/)

# 用途
`elf_loader`能够加载各种elf文件，并留下了扩展功能的接口。它能够被使用在以下地方：
* 在操作系统内核中使用它作为elf文件的加载器
* 使用它实现Rust版本的动态链接器
* 在嵌入式设备上使用它加载elf动态库  
......

# 优势
### ✨ 可以在 `no_std` 环境中工作 ✨
`elf_loader`不依赖Rust `std`，也不强制依赖`libc`和操作系统，因此它可以在内核和嵌入式设备等`no_std`环境中使用。

### ✨ 体积小 ✨
`elf_loader`的体积非常小。基于`elf_loader`实现的[mini-loader](https://github.com/weizhiao/rust-elfloader/tree/main/mini-loader)编译后的二进制文件大小仅为**26K**。下面是使用`bloat`工具分析二进制文件得到的结果：
```shell
cargo bloat --crates --release --target=x86_64-unknown-none -Zbuild-std=core,alloc,panic_abort -Zbuild-std-features=panic_immediate_abort,optimize_for_size
    Finished `release` profile [optimized] target(s) in 0.28s
    Analyzing target/x86_64-unknown-none/release/mini-loader

 File  .text    Size Crate
23.1%  47.9%  5.9KiB elf_loader
 9.1%  18.9%  2.3KiB alloc
 7.1%  14.8%  1.8KiB core
 3.7%   7.7%    974B [Unknown]
 3.2%   6.7%    854B linked_list_allocator
 1.5%   3.0%    383B compiler_builtins
 0.4%   0.8%    105B __rustc
48.2% 100.0% 12.4KiB .text section size, the file size is 25.7KiB

Note: numbers above are a result of guesswork. They are not 100% correct and never will be.
```

### ✨ 速度快 ✨
本库吸取`musl`和`glibc`里`ld.so`实现的优点，并充分利用了Rust的一些特性（比如静态分发），可以生成性能出色的代码。  
下面是性能测试的结果，你可以在Github Actions中的`bench` job中查看它：
```shell
elf_loader:new          time:   [36.333 µs 36.478 µs 36.628 µs]
Found 9 outliers among 100 measurements (9.00%)
  2 (2.00%) low mild
  2 (2.00%) high mild
  5 (5.00%) high severe
Benchmarking libloading:new
Benchmarking libloading:new: Warming up for 3.0000 s

Benchmarking libloading:new: Collecting 100 samples in estimated 5.2174 s (111k iterations)
Benchmarking libloading:new: Analyzing
libloading:new          time:   [46.348 µs 47.065 µs 47.774 µs]
Found 4 outliers among 100 measurements (4.00%)
  3 (3.00%) high mild
  1 (1.00%) high severe

Benchmarking elf_loader:get
Benchmarking elf_loader:get: Warming up for 3.0000 s
Benchmarking elf_loader:get: Collecting 100 samples in estimated 5.0000 s (476M iterations)
Benchmarking elf_loader:get: Analyzing
elf_loader:get          time:   [10.459 ns 10.477 ns 10.498 ns]
Found 1 outliers among 100 measurements (1.00%)
  1 (1.00%) high severe

Benchmarking libloading:get
Benchmarking libloading:get: Warming up for 3.0000 s
Benchmarking libloading:get: Collecting 100 samples in estimated 5.0002 s (54M iterations)
Benchmarking libloading:get: Analyzing
libloading:get          time:   [93.226 ns 93.369 ns 93.538 ns]
Found 11 outliers among 100 measurements (11.00%)
  7 (7.00%) high mild
  4 (4.00%) high severe
```
需要注意的是elf_loader并不是一个动态链接器，它并不能自动解析动态库的依赖，不过它可以作为动态链接器的底层使用。

### ✨ 非常容易移植，具有良好的可扩展性 ✨
如果你想要移植`elf_loader`，你只需为你的平台实现 `Mmap`和`ElfObject` trait。在实现`Mmap` trait时可以参考`elf_loader`提供的默认实现：[mmap](https://github.com/weizhiao/elf_loader/tree/main/src/mmap)。  
此外你可以使用本库提供的`hook`函数来拓展`elf_loader`的功能实现其他任何你想要的功能，在使用`hook`函数时可以参考`dlopen-rs`里的：[hook](https://github.com/weizhiao/dlopen-rs/blob/main/src/loader/mod.rs)。

### ✨ 提供异步接口 ✨
`elf_loader`提供了加载elf文件的异步接口，这使得它在某些并发加载elf文件的场景下有更高的性能上限。不过你需要根据自己的应用场景实现 `Mmap`和`ElfObjectAsync` trait。比如不使用mmap来直接映射elf文件，转而使用mmap+文件读取的方式（mmap创建内存空间再通过文件读取将elf文件的内容读取到mmap创建的空间中）来加载elf文件，这样就能充分利用异步接口带来的优势。

### ✨ 编译期检查 ✨
利用Rust的生命周期机制，在编译期检查elf文件的依赖库是否被提前销毁，大大提高了安全性。  
比如说有三个被`elf_loader`加载的动态库`a`,`b`,`c`，其中`c`依赖`b`，`b`依赖`a`，如果`a`，`b`中的任意一个在`c` drop之前被drop了，那么将不会程序通过编译。（你可以在[examples/relocate](https://github.com/weizhiao/elf_loader/blob/main/examples/relocate.rs)中验证这一点）

### ✨ 延迟绑定 ✨
`elf_loader`支持延迟绑定，这意味着当一个符号被解析时，它不会被立即解析，而是会在第一次被调用时才被解析。

### ✨ 支持RELR相对重定位格式 ✨
`elf_loader`支持RELR相对重定位格式，有关RELR的详细内容可以看这里：[Relative relocations and RELR](https://maskray.me/blog/2021-10-31-relative-relocations-and-relr)。


# Feature

| 特性            | 描述                                                                                          |
| --------------- | --------------------------------------------------------------------------------------------- |
| fs              | 启用对文件系统的支持                                                                          |
| use-libc        | 该feature在开启`fs`或者`mmap` feature时生效。开启`use-libc`时`elf_loader`会使用`libc`作为后端 |
| use-syscall     | 该feature在开启`fs`或者`mmap` feature时生效。使用`linux syscalls`作为后端                     |
| mmap            | 在加载elf文件时，使用有mmap的平台上的默认实现                                                 |
| version         | 在解析符号时使用符号的版本信息                                                                |
| log             | 启用日志                                                                                      |
| rel             | 将rel作为重定位条目的格式                                                                     |
| portable-atomic | 支持没有native指针大小原子操作的目标                                                          |
| lazy            | 启用延迟绑定                                                                                  |

在没有操作系统的情况下请关闭`fs`，`use-syscall`，`use-libc`和`mmap`这四个feature。

# 指令集支持

| 指令集      | 支持 | 延迟绑定 | 测试       |
| ----------- | ---- | -------- | ---------- |
| x86_64      | ✅    | ✅        | ✅(CI)      |
| aarch64     | ✅    | ✅        | ✅(CI)      |
| riscv64     | ✅    | ✅        | ✅(CI)      |
| riscv32     | ✅    | ✅        | ✅(Manual)  |
| loongarch64 | ✅    | ❌        | ✅(Manual ) |
| x86         | ✅    | ✅        | ✅(CI)      |
| arm         | ✅    | ✅        | ✅(CI)      |

# 示例
## 加载一个简单的动态库

```rust
use elf_loader::load_dylib;
use std::collections::HashMap;

fn main() {
    fn print(s: &str) {
        println!("{}", s);
    }

    // Symbols required by dynamic library liba.so
    let mut map = HashMap::new();
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
    // Load and relocate dynamic library liba.so
    let liba = load_dylib!("target/liba.so")
        .unwrap()
        .easy_relocate([].iter(), &pre_find)
        .unwrap();
    // Call function a in liba.so
    let f = unsafe { liba.get::<fn() -> i32>("a").unwrap() };
    println!("{}", f());
}
```

# 最低编译器版本支持
Rust 1.85.0及以上

# 补充
如果你在使用时遇到任何问题，都可以在github上提出issue，此外十分欢迎任何对elf加载器感兴趣的朋友贡献代码（改进elf_loader本身，增加样例，修改文档中存在的问题都可以）。如果觉得elf_loader对你有帮助的话不妨点个star吧。😊