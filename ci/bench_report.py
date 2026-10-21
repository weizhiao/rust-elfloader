"""Summarize this run's Criterion load and teardown measurements."""
import json
from pathlib import Path
import re
import sys

GROUP = 'load_teardown'
IMPLEMENTATIONS = ('relink', 'libloading')
PHASES = ('load', 'teardown')


def benchmark_id(library, implementation, phase):
    return f'{GROUP}/{library}/{implementation}/{phase}'


def format_time(estimate):
    interval = estimate['confidence_interval']
    return (f"{estimate['point_estimate'] / 1000:.2f} "
            f"[{interval['lower_bound'] / 1000:.2f}, {interval['upper_bound'] / 1000:.2f}]")


root = Path(sys.argv[1])
log = Path(sys.argv[2]).read_text()
libraries = sorted(set(re.findall(r'^workload\t([^\r\n]+)$', log, re.MULTILINE)))
if not libraries:
    raise SystemExit('no selected workloads found in run log')
skipped = {}
for library, implementation, reason in re.findall(
        r'^skip\t([^\t\r\n]+)\t([^\t\r\n]+)\t([^\r\n]+)$', log, re.MULTILINE):
    if library not in libraries or implementation != 'libloading':
        raise SystemExit(f'invalid skip record: {library}/{implementation}')
    skipped[library] = reason
required = {
    benchmark_id(library, implementation, phase)
    for library in libraries for implementation in IMPLEMENTATIONS
    for phase in PHASES
    if implementation != 'libloading' or library not in skipped
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

print('# Load and teardown benchmark\n')
print('Warm page cache; fresh Relink context per iteration. Context creation and '
      'host imports are excluded; Relink teardown includes release and context '
      'destruction. Native teardown is dlclose. '
      'Times: mean [95% CI] in µs. Ratio: Relink / libloading (lower is faster). '
      'Load and teardown are sampled independently.\n')
print('| Library | Phase | Relink (µs) | libloading (µs) | Ratio |')
print('|---|---|---:|---:|---:|')
for library in libraries:
    for phase in PHASES:
        relink = estimates[benchmark_id(library, 'relink', phase)]
        if library in skipped:
            print(f'| {library} | {phase} | {format_time(relink)} | N/A | — |')
        else:
            native = estimates[benchmark_id(library, 'libloading', phase)]
            ratio = relink['point_estimate'] / native['point_estimate']
            print(f'| {library} | {phase} | {format_time(relink)} | '
                  f'{format_time(native)} | {ratio:.2f}× |')
if skipped:
    print('\nNative measurements omitted to avoid timing resident-handle reuse:\n')
    for library, reason in sorted(skipped.items()):
        print(f'- `{library}`: {reason}.')
