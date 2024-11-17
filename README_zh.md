# elf_loader
一个用于加载elf文件的轻量化、可拓展、高性能的库。 

它实现了加载elf文件的通用步骤，并留下了扩展接口，用户可以使用它实现自己的定制化loader。

## Example
### mini-loader
本仓库提供了一个使用elf_loader实现miniloader的例子。miniloader可以加载pie文件，目前只支持`x86_64`。  

加载ls:
```shell 
$ cargo r -r -p mini-loader --target=x86_64-unknown-none -- /bin/ls
```
需要注意的是必须使用release参数编译mini-loader。
### dlopen-rs

[dlopen-rs](https://crates.io/crates/dlopen-rs)也是基于elf_loader库实现的。它实现了dlopen的功能，可以在运行时打开动态库。