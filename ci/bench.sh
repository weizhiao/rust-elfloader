#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
report_dir="${RELINK_BENCH_REPORT_DIR:-target/benchmark-report}"
mkdir -p "$report_dir"
{
  git rev-parse HEAD
  rustc -Vv
  cargo -V
  uname -a
  lscpu
  ldd --version
  dpkg-query -W zlib1g liblz4-1 libzstd1 libsqlite3-0 libc6 libpng16-16t64 libfreetype6 libbrotli1 libbz2-1.0 || true
} > "$report_dir/environment.txt" 2>&1
cargo bench --locked --bench end_to_end --features version -- \
  --warm-up-time "${RELINK_BENCH_WARMUP:-1}" \
  --measurement-time "${RELINK_BENCH_DURATION:-2}" \
  --sample-size "${RELINK_BENCH_SAMPLES:-30}" "$@" 2>&1 | tee "$report_dir/run.log"
python3 - "$report_dir/run.log" >> "$report_dir/environment.txt" <<'PY'
import hashlib, pathlib, sys
files = set()
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    if line.startswith('manifest\t'):
        _, kind, name, path = line.split('\t')
        files.add((kind, name, path))
if not files:
    raise SystemExit('no library manifest found')
for kind, name, path in sorted(files):
    data = pathlib.Path(path).read_bytes()
    print(kind, name, path, len(data), hashlib.sha256(data).hexdigest())
PY
python3 ci/bench_report.py target/criterion "$report_dir/run.log" > "$report_dir/summary.md"
cat "$report_dir/summary.md"
