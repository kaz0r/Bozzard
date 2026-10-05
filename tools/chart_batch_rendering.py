#!/usr/bin/env python3
"""Rebuild the batch-rendering proof chart from recorded release measurements.

Requires Matplotlib; it is only a documentation tool, not a runtime dependency.
"""
import json
import os
import tempfile
from pathlib import Path
os.environ.setdefault("MPLCONFIGDIR", str(Path(tempfile.gettempdir()) / "bozzard-batch-matplotlib"))
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / 'docs/measurements/batch-rendering-optimizations/summary.json'
OUT = ROOT / 'docs/images/batch-rendering-optimizations'
OUT.mkdir(parents=True, exist_ok=True)
report = json.loads(DATA.read_text())
workloads = ['active_factory', 'frozen_factory', 'camera_only_factory']
labels = ['Active factory', 'Frozen factory', 'Camera only']
plt.rcParams.update({'font.family': 'DejaVu Sans', 'font.size': 11,
                     'axes.spines.top': False, 'axes.spines.right': False,
                     'axes.titleweight': 'bold', 'text.color': '#172b3a',
                     'axes.labelcolor': '#334155', 'savefig.facecolor': 'white'})
fig, axes = plt.subplots(1, 2, figsize=(12, 5))
for ax, profile, metric, title in zip(
    axes, ['full', 'graph'], ['total_cpu', 'renderer_cpu'],
    ['Extraction + rendering CPU', 'Rendering CPU (shared scene input)'],
):
    for offset, version, color, label in [
        (-.18, 'baseline', '#db7661', 'Main'),
        (.18, 'optimized', '#249b87', 'Optimized'),
    ]:
        records = [report['comparisons'][profile][w]['metrics'][metric] for w in workloads]
        values = [record['median_of_run_medians_ms'][version] for record in records]
        ranges = np.array([
            [value - min(record['run_medians_ms'][version]) for value, record in zip(values, records)],
            [max(record['run_medians_ms'][version]) - value for value, record in zip(values, records)],
        ])
        x = np.arange(3) + offset
        ax.bar(x, values, width=.32, color=color, label=label, zorder=3,
               yerr=ranges, capsize=3, error_kw={'elinewidth': 1, 'ecolor': '#334155'})
        for position, value, record in zip(x, values, records):
            high = max(record['run_medians_ms'][version])
            ax.text(position, high + .02, f'{value:.3f}', ha='center', va='bottom', fontsize=10)
    ax.set_xticks(np.arange(3), labels)
    ax.set_ylabel('CPU time per frame (ms; lower is better)')
    ax.set_title(title, loc='left', pad=15)
    ax.grid(axis='y', color='#e2e8f0', zorder=0)
    ax.set_ylim(0, ax.get_ylim()[1] * 1.13)
    ax.legend(frameon=False)
fig.suptitle('Batch rendering: measured release CPU comparison', x=.055, ha='left', fontweight='bold')
fig.text(.055, .025, 'Apple M2 Pro / Metal · 1280 × 800 · 3 alternating pairs per profile · whiskers span the 3 run medians\n'
         'Full CPU excludes GPU waiting, simulation, UI and presentation. This is not an FPS or GPU-time comparison.',
         fontsize=9, color='#64748b')
fig.tight_layout(rect=(0, .105, 1, .94))
fig.savefig(OUT / 'cpu-comparison.png', dpi=160)
svg = OUT / 'cpu-comparison.svg'
fig.savefig(svg)
svg.write_text('\n'.join(line.rstrip() for line in svg.read_text().splitlines()) + '\n')
plt.close(fig)
print(OUT / 'cpu-comparison.png')
