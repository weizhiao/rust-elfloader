[![](https://img.shields.io/crates/v/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![](https://img.shields.io/crates/d/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![license](https://img.shields.io/crates/l/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![elf_loader on docs.rs](https://docs.rs/elf_loader/badge.svg)](https://docs.rs/elf_loader)
# elf_loader
`elf_loader`能够从内存、文件加载各种形式的elf文件，包括`Executable file`、`Shared object file`和`Position-Independent Executable file`。  

[文档](https://docs.rs/elf_loader/)

# 用途
`elf_loader`能够加载各种elf文件，并留下了扩展功能的接口。它能够被使用在以下地方：
* 在操作系统内核中使用它作为elf文件的加载器
* 使用它实现Rust版本的动态链接器
* 在嵌入式设备上使用它加载elf动态库  
......

# 特性
### ✨ 可以在 `no_std` 环境中工作 ✨
本库不依赖Rust `std`，也不依赖`libc`（虽然你可以通过feature让它使用libc），因此可以在内核和嵌入式设备等`no_std`环境中使用。

### ✨ 速度快 ✨
本库吸取`musl`和`glibc`里`ld.so`实现的优点，并充分利用了Rust的一些特性（比如静态分发），可以生成性能出色的代码。基于`elf_loader`的[dlopen-rs](https://crates.io/crates/dlopen-rs)性能比`libloading`更好。

### ✨ 非常容易移植，具有良好的可扩展性 ✨
如果你想要移植`elf_loader`，你只需为你的平台实现 `Mmap`和`ElfObject` trait。在实现`Mmap` trait时可以参考`elf_loader`提供的默认实现：[mmap](https://github.com/weizhiao/elf_loader/tree/main/src/mmap)。  
此外你可以使用本库提供的`hook`函数来拓展`elf_loader`的功能实现其他任何你想要的功能，在使用`hook`函数时可以参考`dlopen-rs`里的：[hook](https://github.com/weizhiao/dlopen-rs/blob/main/src/loader/mod.rs)。

### ✨ 轻量化 ✨
在使用最少feature的情况下，本库只依赖 `elf`, `cfg-if`, 和 `bitflags` 这额外的三个库。

### ✨ 提供异步接口 ✨
`elf_loader`提供了加载elf文件的异步接口，这使得它在某些并发加载elf文件的场景下有更高的性能上限。不过你需要根据自己的应用场景实现 `Mmap`和`ElfObjectAsync` trait。比如不使用mmap来直接映射elf文件，转而使用mmap+文件读取的方式（mmap创建内存空间再通过文件读取将elf文件的内容读取到mmap创建的空间中）来加载elf文件，这样就能充分利用异步接口带来的优势。

### ✨ 编译期检查 ✨
利用Rust的生命周期机制，在编译期检查elf文件的依赖库是否被提前销毁，大大提高了安全性。  
比如说有三个被`elf_loader`加载的动态库`a`,`b`,`c`，其中`c`依赖`b`，`b`依赖`a`，如果`a`，`b`中的任意一个在`c` drop之前被drop了，那么将不会程序通过编译。（你可以在[examples/relocate](https://github.com/weizhiao/elf_loader/blob/main/examples/relocate.rs)中验证这一点）

# 特性

| 特性      |  描述  |
| --------- | ----------------- |
| fs        |  启用对文件系统的支持        						|
| use-libc  |  使用libc作为后端，否则直接使用linux syscalls		|
| mmap      |  在加载elf文件时，使用有mmap的平台上的默认实现  	| 
| version   |  在解析符号时使用符号的版本信息     |

# 示例
## 加载一个简单的动态库

```rust
use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};
use std::{collections::HashMap, ptr::null};

fn main() {
    fn print(s: &str) {
        println!("{}", s);
    }

	// liba.so依赖的符号
    let mut map = HashMap::new();
    map.insert("__gmon_start__", null());
    map.insert("__cxa_finalize", null());
    map.insert("_ITM_registerTMCloneTable", null());
    map.insert("_ITM_deregisterTMCloneTable", null());
    map.insert("print", print as _);
    let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
	// 加载动态库liba.so
	let loader = Loader::<MmapImpl>::new();
    let liba = loader
        .easy_load_dylib(ElfFile::from_path("target/liba.so").unwrap())
        .unwrap();
	// 重定位liba.so中的符号
    let a = liba.easy_relocate([].iter(), &pre_find).unwrap();
	// 调用liba.so中的函数a
    let f = unsafe { a.get::<fn() -> i32>("a").unwrap() };
    f();
}
```

## mini-loader
[mini-loader](https://github.com/weizhiao/elf_loader/tree/main/mini-loader)是基于`elf_loader`库实现的。mini-loader可以加载并执行elf文件，目前只支持`x86_64`。  

# 未完成
* 支持更多的CPU指令集（目前只支持AArch64，Riscv64，X86-64）。
* 完善对DT_FLAGS标志位的支持。
* 完善注释和文档。  
* 为示例mini-loader支持更多的指令集。
* 增加测试.
* 使用portable simd进一步优化性能。
......

# 补充
你可以在 GitHub 上提出你在使用过程中遇到的任何问题，此外十分欢迎大家为本库提交代码一起完善`elf_loader`的功能。😊