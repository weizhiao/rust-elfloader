# Performance benchmarks

`load_unload` compares Relink's `Linker` with glibc-backed `libloading`
(`RTLD_NOW | RTLD_LOCAL`) on Linux/glibc.

## Run

```sh
# Ubuntu 24.04; use equivalent runtime packages on other distributions.
sudo apt-get install build-essential zlib1g liblz4-1 libzstd1 libsqlite3-0 \
  libpng16-16t64 libfreetype6 libbrotli1 libbz2-1.0
bash ci/bench.sh

# List candidate SONAMEs without running benchmarks:
bash ci/bench.sh --list-workloads

# Select one installed library by SONAME:
bash ci/bench.sh --workload libzstd.so.1
```

Defaults: `libz.so.1`, `liblz4.so.1`, `libzstd.so.1`, `libsqlite3.so.0`,
`libpng16.so.16`, and `libfreetype.so.6`. Other system-cache SONAMEs are accepted;
missing or incompatible libraries fail. `--list-workloads` lists system-cache
candidates excluding host runtimes; it does not validate architecture, dependencies,
or symbols. Preloaded host runtimes cannot be roots.
For direct `cargo bench`, set `RELINK_BENCH_WORKLOAD` to select one library.

Each case defaults to 1 second warmup, 2 seconds measurement, and 30 samples.
Override with `RELINK_BENCH_WARMUP`, `RELINK_BENCH_DURATION`, and
`RELINK_BENCH_SAMPLES`. Criterion options are forwarded, but avoid `--quiet`
and filters or modes that skip either implementation: reporting requires both
measurements in the current log.

## Measurement scope

Timing includes loading the root and its dependencies, eager relocation,
constructors, finalizers, and unloading. Each loader resolves dependencies itself.
Root lookup, host setup, context creation, and cache parsing happen outside timing;
Relink's dependency cache lookup and file loading happen inside it.

Both implementations keep a fixed set of host runtimes loaded, defined by
`HOST_RUNTIMES` in `support/libraries.rs`. Relink bridges their supported non-TLS
exports, including symbol versions and resolved IFUNCs. Other dependencies load
from ELF files. Relink reuses a host-only context and a system-cache snapshot;
its resolver enables RPATH/RUNPATH but omits environment and default directories.

After measurement, Relink's context must contain only host modules. Native root
absence is checked with `RTLD_NOLOAD` before and after libloading measurement.
There are no library-specific API checks, dependency-path comparisons, or
independent checks of unmapping and native dependency unloading.

These are sequential, warm-page-cache measurements. glibc may choose different
optimized dependency binaries and manages its own cache lifetime. Results do not
cover cold startup, loading another libc, or arbitrary host TLS integration.

## Results and comparisons

`target/benchmark-report/` contains `summary.md`, `run.log`, and `environment.txt`
with toolchain/system details and hashes of the cache, roots, and host runtimes
(not the dependency closure). Raw Criterion results are in `target/criterion`;
benchmark IDs use `load_unload/<SONAME>/<implementation>`. Regenerate baselines
created with the previous `end_to_end_cache` group.

```sh
# Baseline revision:
bash ci/bench.sh --save-baseline before
# Changed revision, using the same harness and machine:
bash ci/bench.sh --baseline before
```

Use the same binaries and an idle machine; do not run measurements concurrently.
Regenerate baselines when the harness changes. Shared CI reports results without
an enforced regression threshold.
