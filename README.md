# Relink: Rust ELF Loader and Runtime Linker

<p align="center">
  <img src="https://raw.githubusercontent.com/weizhiao/Relink/main/docs/assets/logo.svg" width="560" alt="Relink logo">
</p>

<p align="center">
  <a href="https://crates.io/crates/elf_loader"><img src="https://img.shields.io/crates/v/elf_loader.svg" alt="Crates.io"></a>
  <a href="https://crates.io/crates/elf_loader"><img src="https://img.shields.io/crates/d/elf_loader.svg" alt="Crates.io downloads"></a>
  <a href="https://docs.rs/elf_loader"><img src="https://docs.rs/elf_loader/badge.svg" alt="Docs.rs"></a>
  <img src="https://img.shields.io/badge/rust-1.93.0%2B-blue.svg" alt="Minimum supported Rust version">
  <a href="https://github.com/weizhiao/Relink/actions/workflows/rust.yml"><img src="https://github.com/weizhiao/Relink/actions/workflows/rust.yml/badge.svg" alt="Build status"></a>
  <img src="https://img.shields.io/crates/l/elf_loader.svg" alt="MIT/Apache-2.0 license">
</p>

<p align="center">
  <a href="README.md">English</a> | <a href="README_zh.md">简体中文</a> |
  <a href="CONTRIBUTING.md">Contributing</a> | <a href="CONTRIBUTING_zh.md">贡献指南</a>
</p>

<p align="center">
  <strong>Load, link, and rewrite ELF in no_std environments.</strong>
</p>

Relink is a highly customizable, high-performance Rust ELF loading and runtime linking library. It can load `.so` files, executables, and object files from disk or memory, then resolve dependencies, apply relocations, and look up symbols. It also exposes observer hooks for load, relocation, symbol binding, and lifecycle events.

## When To Use It

- Load plugins, JIT artifacts, or hot-reload modules at runtime.
- Control `DT_NEEDED` dependencies, symbol scopes, or relocation handling yourself.
- Track or customize load, relocation, symbol binding, and init/fini flow through observer hooks.
- Load ELF from memory, or plug in your own mmap or memory-management backend.
- Scan dependencies and sections first, then reorder layout, pack hot code, use huge pages, or run custom handling.
- Load relocatable ELF files such as `.o` / `.ko`.
- Keep ELF loading available in `no_std`, kernels, embedded systems, or non-standard runtimes.

## What It Loads

- Shared objects / dynamic libraries (`ET_DYN`)
- Executables and PIE-style images (`ET_EXEC`, plus executable-style `ET_DYN`)
- Relocatable object files (`ET_REL`, for example `.o` / `.ko`)

## Compared With `dlopen`

| Capability | Relink | `dlopen`-style loading |
| --- | --- | --- |
| In-memory loading | ✅ Load from paths, byte buffers, or already parsed ELF inputs | ❌ |
| `ET_REL` loading | ✅ Load and relocate `.o` / `.ko` / `ET_REL` files | ❌ |
| Pre-link planning | ✅ Resolve dependencies and sections first, then decide how to map | ❌ |
| Section reordering | ✅ On `x86_64`, adjust section layout before mapping for hot-code packing or custom reordering | ❌ |
| Mapping policy | ✅ Replace mmap, page size, permissions, and memory-access backends | ❌ |
| Dependency and symbol policy | ✅ Customize `DT_NEEDED` resolution, symbol scopes, and relocation interception | ❌ |
| Observer events | ✅ Hook load, symbol binding, relocation, and related stages | ❌ |
| Context isolation | ✅ Multiple `LinkContext`s keep independent modules, dependency graphs, and symbol scopes | ❌ |
| Remote / heterogeneous loading | ✅ Use custom memory access to load remote devices or heterogeneous target ELFs locally | ❌ |

## Quick Start

The default feature set is `full`. It includes the libc backend, relocatable-object loading,
and the built-in native lazy binder:

```toml
[dependencies]
elf_loader = "0.17.0"
```

For a smaller build, disable default features and opt in only to what you need:

```toml
[dependencies]
elf_loader = { version = "0.17.0", default-features = false, features = ["libc"] }
```

### Use Linker to Load Dependencies

```rust
use elf_loader::{
    LinkContext, Linker, Result,
    input::PathBuf,
    linker::{LdCache, SearchConfig, SearchPathResolver},
};

const LINKER: Linker = Linker::new();

fn main() -> Result<()> {
    let root = PathBuf::from("path/to/plugin.so");
    let mut context = LinkContext::for_process();
    // Supply paths appropriate for your target system.
    let config = SearchConfig::new()
        .library_path("$ORIGIN/plugins:/opt/app/lib", "/opt/app/bin")
        .cache(LdCache::load_system()?)
        .default_dirs(["/lib", "/usr/lib"]);
    let resolver = SearchPathResolver::with_config(config);

    let loaded = LINKER
        .resolver(resolver)
        .run()
        .load(&mut context, root)?;

    let run = unsafe {
        context
            .module(loaded.root())?
            .get::<extern "C" fn() -> i32>("run")
            .expect("symbol `run` not found")
    };
    let _ = run();
    drop(loaded.release(&mut context)?);

    Ok(())
}
```

## Benchmarks

`bash ci/bench.sh` compares Relink with libloading using installed zlib, LZ4,
Zstd, SQLite, PNG and FreeType libraries and their real dependencies. It times
loading and complete unloading; symbol lookup and function calls validate
correctness before timing.
See [the benchmark guide](benches/README.md) for the timing boundaries,
host-runtime bridge, system-cache snapshot, validation and CI artifacts.

## Feature Flags

| Feature | Default | Purpose |
| --- | --- | --- |
| `libc` | Yes | Use the libc backend on Linux and Android |
| `lazy-binding` | Yes | Enable the built-in native PLT/GOT lazy binder |
| `object` | Yes | Enable relocatable object (`ET_REL`) loading and `LoaderRun::load_object()` |
| `version` | No | Enable version-aware symbol lookup such as `get_version()` |
| `log` | No | Enable `log` integration for loader and relocation diagnostics |
| `portable-atomic` | No | Support targets without native pointer-sized atomics |
| `use-syscall` | No | Use the Linux syscall backend instead of libc |
| `full` | Yes | Convenience bundle: `lazy-binding`, `object`, `libc` |

Notes:

- TLS requires no feature flag; use `DefaultTlsResolver` or a custom resolver.
- `lazy-binding` only enables the built-in implementation; custom `LazyBinder` implementations do not require this feature.

## Platform Support

| Instruction set | Dynamic libraries / executables | Section reordering | `.o` / `ET_REL` |
| --- | --- | --- | --- |
| `x86_64` | ✅ | ✅ | ✅ |
| `x86` | ✅ | 🚧 | 🚧 |
| `aarch64` | ✅ | 🚧 | 🚧 |
| `arm` | ✅ | 🚧 | 🚧 |
| `riscv64` | ✅ | 🚧 | ✅ |
| `riscv32` | ✅ | 🚧 | 🚧 |
| `loongarch64` | ✅ | 🚧 | 🚧 |
| `xtensa` | 🔧 | 🚧 | 🚧 |

Legend: ✅ supported, 🔧 basic support, 🚧 pending. Section-reorder repair is currently available on `x86_64`; `.o` / `ET_REL` relocation is available on `x86_64` and `riscv64`. Contributions for the other architectures are welcome.

Xtensa currently supports dynamic relocation, but not lazy binding or TLS
relocations.

## Contributing

Issues and pull requests are welcome. Star the project if it is useful in your work.

## License

This project is dual-licensed under either of the following:

- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

## Contributors

<a href="https://github.com/weizhiao/Relink/graphs/contributors">
  <img src="https://contributors-img.web.app/image?repo=weizhiao/Relink" alt="Project contributors">
</a>
