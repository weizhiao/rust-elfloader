# Performance benchmarks

Requires Linux/glibc and libraries listed in `/etc/ld.so.cache`.

## CI comparison

Compare Relink with `libloading` (`RTLD_NOW | RTLD_LOCAL`):

```sh
bash ci/bench.sh                          # zlib, LZ4, Zstd, SQLite, libpng, FreeType
bash ci/bench.sh --list-workloads         # Candidate SONAMEs
bash ci/bench.sh --workload libLLVM.so.21.1
bash ci/bench.sh --save-baseline before
bash ci/bench.sh --baseline before
```

Only load and teardown are benchmarked; total has no separate sampling pass.
Each phase defaults to 1 s warmup, 2 s measurement and 30 samples. Override with
`RELINK_BENCH_WARMUP`, `RELINK_BENCH_DURATION` and `RELINK_BENCH_SAMPLES`.
Criterion options are forwarded; avoid `--quiet` or filters that omit phases.
Reports go to `target/benchmark-report/`, raw data to `target/criterion/`.

## Relink profiling

```sh
CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release --example load_bench --features version
target/release/examples/load_bench --workload libLLVM.so.21.1
perf record -e cpu-clock -F 999 --call-graph dwarf -- \
  target/release/examples/load_bench --workload libLLVM.so.21.1 \
  --iterations 1000 --rounds 1
perf report
```

Defaults: `--warmup-ms 1000 --sample-ms 1000 --rounds 9`; `--iterations N`
overrides calibration. Output includes average `load_ns`, `teardown_ns` and
`total_ns` per iteration, plus P10/median/P90 across round averages.

## Timing

Every Relink iteration uses a fresh context with fixed host runtimes:

- **Load:** library/dependency loading, relocation and constructors.
- **Teardown:** release and destruction of released modules, including finalizers and unmapping.

Context creation/destruction, host imports and cache setup are excluded. The profiling runner
also reports total as load + teardown from the same iteration, without extra sampling.

These are warm-file-cache measurements. Native timing uses `dlopen`/`dlclose`;
its namespace persists and dependencies may remain resident. If the root stays
loaded, as with LLVM's `NODELETE`, CI reports native results as N/A. Relink still
measures release; any modules retained until context destruction are cleaned up
outside timing.

Criterion samples load and teardown independently. `perf` also captures preparation
outside timed regions.
Compare on an idle machine with the same CPU affinity. Regenerate baselines after
harness changes, including the exclusion of context destruction from teardown; the current `load_teardown` group differs from old `load_unload` results.
