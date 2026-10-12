# Performance benchmarks

The `end_to_end` executable measures **unmodified installed zlib, LZ4, Zstd,
SQLite, libpng, and FreeType libraries together with their ordinary dependencies**.
It compares Relink's `Linker` with `libloading` backed by glibc
`dlopen(RTLD_NOW | RTLD_LOCAL)`.

```sh
# Ubuntu 24.04, Linux/glibc; use equivalent runtime packages on other distributions.
sudo apt-get install build-essential zlib1g liblz4-1 libzstd1 libsqlite3-0 \
  libpng16-16t64 libfreetype6 libbrotli1 libbz2-1.0
bash ci/bench.sh
```

The script records Rust/compiler/CPU/OS/package details and SHA-256 hashes of
the cache file, all recorded real libraries and host runtimes in
`target/benchmark-report/environment.txt`, output and dependency edges in
`run.log`, and a comparison table (including real module counts) in `summary.md`.
Criterion's raw samples and confidence intervals live in `target/criterion`.

## Real dependency graphs

The harness recursively discovers `DT_NEEDED` entries from the installed files,
without loading them into either loader during discovery. For example:

```text
libpng -> zlib
FreeType -> zlib, libbz2, libpng -> zlib,
            libbrotlidec -> libbrotlicommon
```

The exact graph depends on distribution packaging. For example, some LZ4 builds
also depend on xxHash; that dependency is loaded from its real file as well.
No particular dependency graph is required: the files' actual `DT_NEEDED`
entries determine what is loaded. The report records the resulting module counts.

Only the explicit allowlist in `support/real_libraries.rs::is_host_runtime` can use a
synthetic module: libc, libm, libdl, libpthread, librt, libgcc_s and the listed
platform dynamic loaders. The suite bridges only the runtimes needed by the
combined fixtures and their host dependencies; ordinary libraries never fall
back to a synthetic module. On the development machine this means libc, libm
and ld-linux. Both implementations keep these host libraries loaded.

The host bridge is built once before timing and shared by every fixture. It
exports the runtimes' full visible global/weak function and object tables,
including GNU symbol versions and their default/non-default distinction.
`dlsym`/`dlvsym` resolve IFUNCs to function addresses. ELF version-marker symbols
are excluded. TLS exports are not bridged, and TLS imports fail explicitly.
The tables are **not trimmed to each target's imports**, although synthetic
lookup still differs from glibc's native hash-table lookup.

The harness reads `/etc/ld.so.cache` directly through Relink's `LdCache` API;
it does not execute `ldconfig -p`. The same complete snapshot locates fixtures
and supplies candidates to the glibc-style `SearchPathResolver::with_config` preset
during loading. The benchmark supplies the cache only: RPATH/RUNPATH ordering
is enabled, but environment and default-directory inputs are omitted.
No fixture-specific dependency directory list is precomputed. Actual loaded
paths are checked so both loaders use the same binaries.

The cache snapshot is read, parsed and indexed once before timing, then shared
across all cases. Every Relink load times SONAME lookup and candidate file
opening/ELF validation against that full snapshot. glibc manages its own cache
mapping lifetime, which can differ; these results do not claim identical cache
initialization costs. Hardware-specific cache entries are conservatively
skipped by Relink. If glibc selects a different optimized binary, the file-set
validation fails rather than silently comparing different implementations.

## What is timed and validated

- `load_unload`: opening, mapping, ELF parsing, dependency resolution, eager
  relocation, ELF initialization, finalization, and unmapping of the root and
  its ordinary dependency closure. Relink also maintains the module graph and
  address index. One persistent context with host modules is reused across
  iterations, as in a long-running embedding application.

Dependency discovery, building the host tables, constructing the context and
resolver, and correctness checks (including exported-symbol lookup and actual
function calls) run outside timing. Relink still reads and
links the real dependency files on every iteration; it does not reuse parsed
ELF images from discovery. The native loader likewise reloads the entire
ordinary dependency group.

Before measurement the loaders are validated separately, with no native copy
of the target graph present while Relink is checked:

- Check exact canonical file sets for the native graph using `dl_iterate_phdr`
  and for Relink using its committed module metadata. Check Relink's module
  count to reject duplicate shared dependencies.
- Call version functions for the original fixtures. Decode a generated 1x1 PNG
  to known RGBA bytes in the PNG fixture; also check FreeType's `FT_MulFix`
  result without assuming it was built with PNG support.
- Do not call `FT_Init_FreeType`: some builds dynamically load optional HarfBuzz
  from that application API, potentially retaining a native FreeType through a
  dependency cycle. This suite measures the static `DT_NEEDED` graph, not
  application-created font engines or optional runtime `dlopen` workloads.
- Check that releasing Relink unloads exactly the real graph and leaves only
  host modules. Check for unexpected native library loads during validation.
- Use `RTLD_NOLOAD` for **every ordinary dependency**, before measurement and
  after each implementation's measured case, to reject cached native handles.

Missing libraries, failed calls, graph/path mismatches and retained native
libraries fail the run. No workload is silently skipped.

These are warm-page-cache measurements, without privileged cache dropping.
They do not measure loading a second libc, cold process startup, arbitrary host
TLS integration, or full glibc namespace semantics. Compare revisions on the
same idle machine with the same recorded binaries and benchmark harness. The
suite uses the repository's normal release/bench optimization policy.

The Criterion group is now `end_to_end_cache`. The earlier `end_to_end_graph`
group used fixture-derived search directories, and `end_to_end` used
per-fixture import subsets and preloaded ordinary libraries. Neither is a
valid baseline for this workload; the report ignores both groups.

## Comparing revisions

```sh
# Run on the baseline source, with this benchmark harness present:
bash ci/bench.sh --save-baseline before
# After changing the implementation, on the same machine:
bash ci/bench.sh --baseline before
```

Do not run two measurements concurrently. For a longer local run, set
`RELINK_BENCH_WARMUP=3 RELINK_BENCH_DURATION=5 RELINK_BENCH_SAMPLES=50`.
The defaults are 1 second warmup, 2 seconds measurement, and 30 samples per
case (12 cases total). The implementations still run sequentially in one
process; these are not randomized paired multi-process measurements.
Shared CI runners report measurements and artifacts without a hard
regression threshold: scheduling/frequency noise can otherwise fail good PRs.
The workflow runs on relevant PRs, pushes to main, and manual dispatch.
It does not claim a same-machine before/after comparison across separate jobs.
