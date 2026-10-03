#!/usr/bin/env python3
"""Rebuild the committed PNG performance figures with Matplotlib.

Install matplotlib in a virtualenv, then run this script from any directory.
Inputs are recorded samples, not generated or estimated benchmark timings.
"""
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / 'docs/measurements/further-optimizations'
OUT = ROOT / 'docs/images/further-optimizations'
OUT.mkdir(parents=True, exist_ok=True)
plt.rcParams.update({'font.family': 'DejaVu Sans', 'font.size': 11,
                     'axes.spines.top': False, 'axes.spines.right': False,
                     'axes.spines.left': False, 'axes.titleweight': 'bold',
                     'axes.labelcolor': '#334155', 'text.color': '#172b3a',
                     'xtick.color': '#334155', 'ytick.color': '#334155',
                     'savefig.facecolor': 'white', 'figure.facecolor': 'white'})
OLD, NEW, BLUE = '#db7661', '#249b87', '#5079c5'

def save(fig, name, note):
    fig.text(.02, .015, note, fontsize=9, color='#64748b')
    fig.tight_layout(rect=(0, .07, 1, 1))
    fig.savefig(OUT / name, dpi=160)
    plt.close(fig)

def bars(title, labels, values, colors, ylabel, name, note, budget=None):
    fig, ax = plt.subplots(figsize=(10.5, 4.8))
    x = np.arange(len(labels))
    ax.bar(x, values, color=colors, width=.62, zorder=3)
    ax.set_xticks(x, labels)
    ax.set_ylabel(ylabel)
    ax.set_title(title, loc='left', pad=20)
    ax.set_ylim(0, max(max(values) * 1.25, (budget or 0) * 1.2))
    ax.grid(axis='y', color='#e2e8f0', zorder=0)
    for i, value in enumerate(values):
        ax.text(i, value + ax.get_ylim()[1]*.025, f'{value:.2f}', ha='center', fontweight='bold')
    if budget:
        ax.axhline(budget, color='#334155', ls='--', lw=1.2, label=f'60 Hz tick budget: {budget:.2f} ms')
        ax.legend(frameon=False, loc='upper right')
    save(fig, name, note)

editors = json.loads((DATA/'editor.json').read_text())
by_label = {r['label']: r for r in editors}
labels = ['Original\ndevelopment', 'Virtualized\nwidgets', 'Optimized egui\ndevelopment', 'Release\ntitle', 'Release\nproduction demo']
keys = ['editor-before', 'editor-after', 'editor-optimized', 'editor-release', 'editor-release-world-final']
bars('Editor CPU time across measured configurations', labels,
     [by_label[k]['cpu_median_ms'] for k in keys], [OLD, BLUE, BLUE, NEW, NEW],
     'Median editor CPU time (ms)', 'editor-cpu.png',
     'RTX 3060 / Ryzen 9 5950X · 120–180 frames · exploratory runs; viewport sizes differ. Lower is better.')

import statistics
fig, ax = plt.subplots(figsize=(9.8, 4.7))
names = ['Pane Hierarchy','Pane Assets']
for i, (key, label, color) in enumerate([('editor-panes','Before (development)',OLD),('editor-optimized','After (development)',NEW)]):
    row = by_label[key]
    values = [statistics.median(f['pane_ms'].get(n, 0) for f in row['frame_samples']) for n in names]
    x = np.arange(2) + (i-.5)*.34
    ax.bar(x, values, width=.3, color=color, label=label, zorder=3)
    for at, value in zip(x, values): ax.text(at, value+1.2, f'{value:.1f} ms', ha='center', fontsize=11)
ax.set_xticks(np.arange(2), ['Hierarchy', 'Content Browser'])
ax.set_ylabel('Median pane CPU time (ms)')
ax.set_title('Only visible widgets need layout and painting', loc='left', pad=20)
ax.set_ylim(0, 88);ax.grid(axis='y',color='#e2e8f0',zorder=0);ax.legend(frameon=False)
save(fig,'editor-panes.png','Same 782-object / 307-asset project · 90 before / 180 after frames · viewport/layout caveats in the report.')

pack = json.loads((DATA/'pack.json').read_text())
bars('Compressed game content', ['Loose staged\ncontent', 'gamepack.bpack'],
     [pack['uncompressed_bytes']/2**20,pack['packed_bytes']/2**20], [OLD,NEW],
     'Content size (MiB)', 'gamepack-size.png',
     f"{pack['files']} files, including {pack['scripts']} scripts · runtime executable excluded · no encryption or FPS claim.")

runs = json.loads((DATA/'simulation.json').read_text())
labels = ['Original', 'Pages read\nonce', 'Native row\ngather', 'Unchanged\ncolumns skipped', 'Native sparse\npower codec']
keys = ['baseline','page-once','gather-only','skip-columns','final']
values = [statistics.median(r['p95_ms'] for r in runs if r['configuration']==k) for k in keys]
bars('Dense factory: long simulation ticks become shorter', labels, values,
     [OLD,BLUE,BLUE,BLUE,NEW], '95th-percentile fixed-tick time (ms)', 'simulation-p95.png',
     'Same release binary and save · 60 warmups + 180 samples/run · 3 baseline/final trials; 1 per intermediate · lower is better.',1000/60)

fig, ax = plt.subplots(figsize=(10.5,4.8))
for key, label, color in [('baseline','Original scripts',OLD),('final','Optimized scripts',NEW)]:
    values = sorted(v for r in runs if r['configuration']==key for v in r['tick_ms'])
    ax.plot(values, np.arange(1,len(values)+1)/len(values)*100, label=label, color=color, lw=2.5)
ax.axvline(1000/60,color='#334155',ls='--',lw=1.2,label='60 Hz tick budget')
ax.set_xlabel('Fixed-tick CPU time (ms)');ax.set_ylabel('Samples at or below time (%)')
ax.set_title('Simulation latency distribution across three trials',loc='left',pad=20)
ax.set_ylim(0,102);ax.grid(color='#e2e8f0');ax.legend(frameon=False,loc='lower right')
save(fig,'simulation-distribution.png','540 measured ticks per configuration · headless CPU stepping; these are not GPU timings or presented FPS.')

native = json.loads((DATA/'player-final.json').read_text())
fig, ax = plt.subplots(figsize=(10.5,4.8))
for i,(key,label,color) in enumerate([('baseline','Original scripts',OLD),('final','Optimized scripts',NEW)]):
    values=[native[key][kind]['p99_ms'] for kind in ['player_cpu_frame','player_presentation_interval']]
    x=np.arange(2)+(i-.5)*.34
    ax.bar(x,values,width=.3,color=color,label=label,zorder=3)
    for at,value in zip(x,values): ax.text(at,value+1,f'{value:.2f} ms',ha='center')
ax.set_xticks(np.arange(2),['Whole native CPU frame','Presented frame interval'])
ax.set_ylabel('99th-percentile time (ms)');ax.set_title('Native play: shorter stalls with the same geometry',loc='left',pad=20)
ax.set_ylim(0,54);ax.grid(axis='y',color='#e2e8f0',zorder=0);ax.legend(frameon=False)
save(fig,'player-tail.png','600 native frames/run, including startup · 349,668 loaded triangles / 263 draws · medians remain near the refresh limit.')
