"""Summarize this run's Criterion load/unload measurements."""
import json
from pathlib import Path
import re
import sys

GROUP = 'load_unload'
IMPLEMENTATIONS = ('relink', 'libloading')


def benchmark_id(library, implementation):
    return f'{GROUP}/{library}/{implementation}'


def format_time(estimate):
    interval = estimate['confidence_interval']
    return (f"{estimate['point_estimate'] / 1000:.2f} "
            f"[{interval['lower_bound'] / 1000:.2f}, {interval['upper_bound'] / 1000:.2f}]")


root = Path(sys.argv[1])
log = Path(sys.argv[2]).read_text()
libraries = sorted(set(re.findall(r'^workload\t([^\r\n]+)$', log, re.MULTILINE)))
if not libraries:
    raise SystemExit('no selected workloads found in run log')
required = {
    benchmark_id(library, implementation)
    for library in libraries for implementation in IMPLEMENTATIONS
}
# Old result files alone must not count as measurements from this run.
analyzed = set(re.findall(r'Benchmarking ([^\r\n]+): Analyzing', log))
if missing := required - analyzed:
    raise SystemExit(f'incomplete measurements in this run: {sorted(missing)}')

estimates = {}
for path in root.glob('**/new/benchmark.json'):
    full_id = json.loads(path.read_text())['full_id']
    if full_id in required:
        if full_id in estimates:
            raise SystemExit(f'duplicate benchmark result: {full_id}')
        estimates[full_id] = json.loads(path.with_name('estimates.json').read_text())['mean']
if missing := required - estimates.keys():
    raise SystemExit(f'missing benchmark results: {sorted(missing)}')

print('# Load/unload benchmark\n')
print('Warm page cache; sequential measurements with preloaded host runtimes. '
      'Times: mean [95% CI] in µs. Ratio: Relink / libloading (lower is faster).\n')
print('| Library | Relink (µs) | libloading (µs) | Ratio |')
print('|---|---:|---:|---:|')
for library in libraries:
    relink, native = (estimates[benchmark_id(library, impl)] for impl in IMPLEMENTATIONS)
    ratio = relink['point_estimate'] / native['point_estimate']
    print(f'| {library} | {format_time(relink)} | {format_time(native)} | {ratio:.2f}× |')
