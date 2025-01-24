[![](https://img.shields.io/crates/v/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![](https://img.shields.io/crates/d/elf_loader.svg)](https://crates.io/crates/elf_loader)
[![license](https://img.shields.io/crates/l/elf_loader.svg)](https://crates.io/crates/elf_loader)
# elf_loader
一个用于加载elf文件的轻量化、可拓展、高性能的库。  

[文档](https://docs.rs/elf_loader/)

# 特性
### ✨ 可以在 `no_std` 环境中工作 ✨
此包提供了一个不使用任何 std 特性的 elf 加载接口，因此可以在内核和嵌入式设备等`no_std`环境中使用。

### ✨ 速度快 ✨
该crate充分利用了rust的一些特性，可以生成性能优异的代码。

### ✨ 非常容易移植，具有良好的可扩展性 ✨
如果您想要移植此 crate，则只需为您的平台实现 `Mmap` 特征即可，并且您可以使用hook函数基于此 crate 实现其他功能。

### ✨ 轻量化 ✨
在使用最少feature的情况下，本库只依赖 `elf`, `cfg-if`, 和 `bitflags` 这额外的三个库。

# 用途
它实现了加载elf文件的通用步骤，并留下了扩展接口，用户可以使用它实现自己的定制化loader。

# 示例
## mini-loader
本仓库提供了一个使用`elf_loader`实现[mini-loader](https://github.com/weizhiao/elf_loader/tree/main/mini-loader)的例子。miniloader可以加载pie文件，目前只支持`x86_64`。  

加载ls:
```shell 
$ cargo build --release -p mini-loader --target=x86_64-unknown-none
$ ./mini-loader /bin/ls
```
需要注意的是必须使用release参数编译mini-loader。

## dlopen-rs
[dlopen-rs](https://crates.io/crates/dlopen-rs)也是基于`elf_loader`库实现的。它实现了dlopen的功能，可以在运行时打开动态库。