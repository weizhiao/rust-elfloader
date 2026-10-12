"""Summarize the real-library Criterion measurements (stdlib only)."""
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
rows = {}
for benchmark in root.glob('**/new/benchmark.json'):
    metadata = json.loads(benchmark.read_text())
    parts = metadata['full_id'].split('/')
    if len(parts) != 4 or parts[0] != 'end_to_end_cache' or parts[2] != 'load_unload':
        continue
    estimate = json.loads(benchmark.with_name('estimates.json').read_text())['mean']
    rows.setdefault(tuple(parts[1:3]), {})[parts[3]] = estimate
expected = {
    (library, 'load_unload')
    for library in ('libz.so.1', 'liblz4.so.1', 'libzstd.so.1', 'libsqlite3.so.0', 'libpng16.so.16', 'libfreetype.so.6')
}
if set(rows) != expected:
    raise SystemExit(f'incomplete suite: missing={expected - set(rows)}, unexpected={set(rows) - expected}')
counts = dict(re.findall(r'^validated ([^:]+): .*; real_modules=(\d+)$', pathlib.Path(sys.argv[2]).read_text(), re.MULTILINE))
if set(counts) != {library for library, _ in expected}:
    raise SystemExit('missing validated dependency graph in run log')
print('# Real-library end-to-end benchmark\n')
print('Warm OS page cache; root and all ordinary dependencies are loaded and unloaded each iteration. '
      'Only allowlisted host runtimes are already loaded in both implementations. '
      'Relink uses full non-TLS host export tables and a preloaded system-cache snapshot. '
      'Cache lookup is timed; reading and parsing the snapshot is not.\n')
print('Times are means in microseconds with 95% confidence intervals. '
      'The ratio is Relink / libloading; lower is faster. '
      'These are sequential Criterion measurements, not paired multi-process comparisons.\n')
print('| Library | Real modules | Operation | Relink (µs) | libloading (µs) | Ratio |')
print('|---|---:|---|---:|---:|---:|')
for (library, operation), estimates in sorted(rows.items()):
    if set(estimates) != {'relink', 'libloading'}:
        raise SystemExit(f'incomplete benchmark pair: {library}/{operation}')
    def cell(value):
        interval = value['confidence_interval']
        return (f"{value['point_estimate'] / 1000:.2f} "
                f"[{interval['lower_bound'] / 1000:.2f}, {interval['upper_bound'] / 1000:.2f}]")
    relink, native = estimates['relink'], estimates['libloading']
    ratio = relink['point_estimate'] / native['point_estimate']
    print(f'| {library} | {counts[library]} | {operation} | {cell(relink)} | {cell(native)} | {ratio:.2f}× |')
