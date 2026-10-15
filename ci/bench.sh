#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
criterion_args=()
workload=""
list_workloads=false
while (($#)); do
  case "$1" in
    --list-workloads)
      list_workloads=true
      shift
      ;;
    --workload)
      if (($# < 2)) || [[ -z "$2" || "$2" == --* || -n "$workload" ]]; then
        echo "usage: $0 [--workload <soname>] [Criterion options]" >&2
        exit 2
      fi
      workload="$2"
      shift 2
      ;;
    --workload=*)
      if [[ -n "$workload" || -z "${1#*=}" ]]; then
        echo "specify exactly one non-empty --workload" >&2
        exit 2
      fi
      workload="${1#*=}"
      shift
      ;;
    *) criterion_args+=("$1"); shift ;;
  esac
done
if "$list_workloads"; then
  if [[ -n "$workload" ]] || ((${#criterion_args[@]})); then
    echo "--list-workloads cannot be combined with workload or Criterion options" >&2
    exit 2
  fi
  ldconfig_cmd="$(command -v ldconfig || true)"
  if [[ -z "$ldconfig_cmd" && -x /sbin/ldconfig ]]; then
    ldconfig_cmd=/sbin/ldconfig
  fi
  if [[ -z "$ldconfig_cmd" ]]; then
    echo "ldconfig is required to list system-cache SONAMEs" >&2
    exit 1
  fi
  echo "Candidate SONAMEs only; architecture, dependencies and symbols are not validated." >&2
  # Keep exclusions aligned with HOST_RUNTIMES in benches/support/libraries.rs.
  "$ldconfig_cmd" -p | awk '/=>/ {print $1}' | LC_ALL=C sort -u | \
    awk '$0 !~ /^(libc\.so\.6|libm\.so\.6|libdl\.so\.2|libpthread\.so\.0|librt\.so\.1|libgcc_s\.so\.1|ld-linux-x86-64\.so\.2|ld-linux\.so\.2|ld-linux-aarch64\.so\.1)$/'
  exit 0
fi
export RELINK_BENCH_WORKLOAD="$workload"
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
cargo bench --locked --bench load_unload --features version -- \
  --warm-up-time "${RELINK_BENCH_WARMUP:-1}" \
  --measurement-time "${RELINK_BENCH_DURATION:-2}" \
  --sample-size "${RELINK_BENCH_SAMPLES:-30}" "${criterion_args[@]}" 2>&1 | tee "$report_dir/run.log" | \
  python3 -u -c 'import sys
for line in sys.stdin:
    if not line.startswith("manifest\t"):
        sys.stdout.write(line)
'
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
