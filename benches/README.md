# Performance benchmarks

Requires Linux/glibc and the selected libraries in `/etc/ld.so.cache`.

## CI comparison

Compare Relink with `libloading` (`RTLD_NOW | RTLD_LOCAL`):

```sh
bash ci/bench.sh                          # Default suite
bash ci/bench.sh --list-workloads         # Candidate SONAMEs, not validated
bash ci/bench.sh --workload libzstd.so.1   # One library
bash ci/bench.sh --save-baseline before
bash ci/bench.sh --baseline before
```

The default suite covers zlib, LZ4, Zstd, SQLite, libpng and FreeType.
Each case uses 1 second warmup, 2 seconds measurement and 30 samples; override
with `RELINK_BENCH_WARMUP`, `RELINK_BENCH_DURATION` and `RELINK_BENCH_SAMPLES`.
Criterion options are forwarded; reporting requires both loaders, so avoid
`--quiet` or filters that skip either one.

Reports and environment/file manifests go to `target/benchmark-report/`;
raw Criterion data goes to `target/criterion/`.

## Relink profiling

The standalone runner requires `--workload`. It prints each round’s iteration
count, elapsed time and average ns/load, plus P10/median/P90 across round averages.

```sh
CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release --example load_bench --features version
taskset -c 2 target/release/examples/load_bench --workload libfreetype.so.6
perf record -e cpu-clock -F 999 --call-graph dwarf -- \
  target/release/examples/load_bench --workload libfreetype.so.6 \
  --iterations 10000 --rounds 1
perf report
```

Defaults: `--warmup-ms 1000 --sample-ms 1000 --rounds 9`. Iterations are calibrated
once to target the sample duration; `--iterations N` selects a fixed count instead.
P10/P90 show variation between rounds, not confidence bounds or operation tails.
Compare before/after binaries in alternating order with the same settings and CPU
affinity (choose an available CPU). Profiling also includes setup and calibration.

## Measurement scope

Timing includes root/dependency loading, eager relocation, constructors,
finalizers and unloading. Root lookup, host setup, context creation and cache
parsing are excluded. These are warm-page-cache measurements, not cold startup.

Both loaders retain the fixed `HOST_RUNTIMES` from `support/libraries.rs`.
Relink imports them with `host::open`, including versions, IFUNCs and dynamic TLS;
static host TLS offsets are unsupported. Other dependencies load from ELF files.
Relink uses RPATH/RUNPATH and a cache snapshot, without environment or default
search directories; glibc may select different dependency binaries.

Checks verify Relink's context returns to host-only state and the native root
is absent around native measurements. They do not validate library APIs or
independently verify all dependency unmapping. Use an idle machine, never run
measurements concurrently, and regenerate baselines when the harness changes.
