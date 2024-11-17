# elf_loader
A `lightweight`, `extensible`, and `high-performance` library for loading ELF files.    
It implements the general steps for loading ELF files and leaves extension interfaces, allowing users to implement their own customized loaders.

## Example
### mini-loader
This repository provides an example of a `mini-loader` implemented using `elf_loader`. The miniloader can load PIE files and currently only supports   `x86_64` .

Load `ls`:

```shell
$ cargo r -r -p mini-loader --target=x86_64-unknown-none -- /bin/ls
``` 

### dlopen-rs
[dlopen-rs](https://crates.io/crates/dlopen-rs) is also implemented based on the elf_loader library. It implements the functionality of dlopen, allowing dynamic libraries to be opened at runtime.