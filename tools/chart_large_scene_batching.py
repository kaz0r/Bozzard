#!/usr/bin/env python3
"""Rebuild the large-scene batching charts from recorded measurements.

Standard library only: writes theme-aware SVGs (light and dark through
prefers-color-scheme) to docs/images/large-scene-batching/ from
docs/measurements/large-scene-batching/summary.json. --check verifies them.
"""
import argparse
import json
import math
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "docs/measurements/large-scene-batching/summary.json"
OUT = ROOT / "docs/images/large-scene-batching"

# Emphasis form: main recedes in gray, this pass carries the accent. Validated
# with the dataviz palette checks (separation and contrast in both modes).
STYLE = """
.surface{fill:#fcfcfb}.ink{fill:#0b0b0b}.ink2{fill:#52514e}.muted{fill:#898781}
.grid{stroke:#e1e0d9;stroke-width:1}.axis{stroke:#c3c2b7;stroke-width:1}
.main{fill:#8f8d86}.pass{fill:#2a78d6}.link{stroke:#c3c2b7;stroke-width:2}
.ring{stroke:#fcfcfb;stroke-width:2}
@media (prefers-color-scheme: dark){
.surface{fill:#1a1a19}.ink{fill:#ffffff}.ink2{fill:#c3c2b7}.muted{fill:#898781}
.grid{stroke:#2c2c2a}.axis{stroke:#383835}.main{fill:#6f6d66}.pass{fill:#3987e5}
.link{stroke:#383835}.ring{stroke:#1a1a19}}
text{font-family:system-ui,-apple-system,'Segoe UI',sans-serif}
.title{font-size:16px;font-weight:600}.subtitle{font-size:12px}
.label{font-size:12px}.value{font-size:11px;font-variant-numeric:tabular-nums}
.tick{font-size:11px;font-variant-numeric:tabular-nums}
"""


def number(value, digits=0):
    text = f"{value:,.{digits}f}"
    return text


class Svg:
    def __init__(self, width, height, title, description):
        self.width, self.height = width, height
        self.parts = [
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
            f'viewBox="0 0 {width} {height}" role="img" aria-labelledby="title desc">',
            f"<title id=\"title\">{title}</title>",
            f"<desc id=\"desc\">{description}</desc>",
            f"<style>{STYLE.strip()}</style>",
            f'<rect class="surface" width="{width}" height="{height}" rx="8"/>',
        ]

    def text(self, x, y, content, classes, anchor="start"):
        self.parts.append(f'<text x="{x:.1f}" y="{y:.1f}" class="{classes}" '
                          f'text-anchor="{anchor}">{content}</text>')

    def line(self, x1, y1, x2, y2, classes):
        self.parts.append(f'<line x1="{x1:.1f}" y1="{y1:.1f}" x2="{x2:.1f}" y2="{y2:.1f}" '
                          f'class="{classes}"/>')

    def column(self, x, width, top, base, classes, radius=4):
        """A bar growing up from `base`, rounded only at its data end."""
        r = min(radius, (base - top) / 2, width / 2)
        self.parts.append(
            f'<path class="{classes}" d="M{x:.1f},{base:.1f} V{top + r:.1f} '
            f'Q{x:.1f},{top:.1f} {x + r:.1f},{top:.1f} H{x + width - r:.1f} '
            f'Q{x + width:.1f},{top:.1f} {x + width:.1f},{top + r:.1f} V{base:.1f} Z"/>')

    def bar(self, start, y, end, height, classes, radius=4):
        """A bar growing right from `start`, rounded only at its data end."""
        r = min(radius, (end - start) / 2, height / 2)
        self.parts.append(
            f'<path class="{classes}" d="M{start:.1f},{y:.1f} H{end - r:.1f} '
            f'Q{end:.1f},{y:.1f} {end:.1f},{y + r:.1f} V{y + height - r:.1f} '
            f'Q{end:.1f},{y + height:.1f} {end - r:.1f},{y + height:.1f} H{start:.1f} Z"/>')

    def dot(self, x, y, classes, radius=5):
        self.parts.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{radius}" class="{classes} ring"/>')

    def legend(self, x, y, entries):
        """Swatch + label pairs, laid out right-to-left from `x`."""
        for label, classes, shape in reversed(entries):
            width = 7 * len(label) + 8
            x -= width
            self.text(x + 14, y + 4, label, "label ink2")
            if shape == "dot":
                self.dot(x + 5, y, classes)
            else:
                self.parts.append(f'<rect x="{x:.1f}" y="{y - 5:.1f}" width="10" height="10" '
                                  f'rx="2" class="{classes}"/>')
            x -= 18

    def finish(self):
        return "\n".join(self.parts + ["</svg>"]) + "\n"


def frame_time(data):
    runs = data["benchmark"]["runs"]
    sizes = sorted({run["cubes"] for run in runs})
    by = {(run["cubes"], run["build"]): run for run in runs}
    svg = Svg(760, 354, "Frame cost at 100k and 140k surfaces",
              "Synchronized frame time and renderer CPU per frame for main and this pass. "
              + "; ".join(f"{s // 1000}k cubes: frame time {by[(s, 'main')]['synchronized_ms']:.1f} to "
                          f"{by[(s, 'this pass')]['synchronized_ms']:.1f} ms, CPU "
                          f"{by[(s, 'main')]['cpu_ms']:.1f} to {by[(s, 'this pass')]['cpu_ms']:.1f} ms"
                          for s in sizes))
    svg.text(24, 32, "Frame cost at 100k–140k surfaces", "title ink")
    svg.text(24, 52, f"{data['device']} · median of 60 warm frames · lower is better",
             "subtitle ink2")
    svg.legend(736, 32, [("main (46e4f2f)", "main", "box"), ("this pass", "pass", "box")])
    panels = [("Synchronized frame time (ms)", "synchronized_ms"), ("Renderer CPU (ms)", "cpu_ms")]
    top, base = 104, 268
    ceiling = 250
    for index, (title, metric) in enumerate(panels):
        left = 64 + index * 356
        right = left + 300
        svg.text(left - 40, 84, title, "label ink")
        for tick in range(0, ceiling + 1, 50):
            y = base - (base - top) * tick / ceiling
            svg.line(left, y, right, y, "grid" if tick else "axis")
            svg.text(left - 8, y + 4, number(tick), "tick muted", "end")
        for group, size in enumerate(sizes):
            center = left + 75 + group * 150
            for offset, build, classes in ((-25, "main", "main"), (1, "this pass", "pass")):
                value = by[(size, build)][metric]
                y = base - (base - top) * value / ceiling
                svg.column(center + offset, 24, y, base, classes)
                svg.text(center + offset + 12, y - 6, number(value, 1), "value ink2", "middle")
            before, after = by[(size, "main")][metric], by[(size, "this pass")][metric]
            svg.text(center, base + 18, f"{size // 1000}k cubes", "label ink2", "middle")
            svg.text(center, base + 34, f"−{100 * (1 - after / before):.0f}%", "value ink", "middle")
    svg.text(24, 324, "Tinted cube field, floor, orbiting perspective camera, one moving caster, "
             "2048² sun, 1280 × 720.", "subtitle muted")
    svg.text(24, 341, "Synchronized frame time spans draw plus GPU completion; renderer CPU excludes "
             "the GPU.", "subtitle muted")
    return svg.finish()


def work_reduction(data):
    rows = data["fixtures"]
    height = 112 + 44 * len(rows) + 36
    svg = Svg(760, height, "Per-frame work, before and after",
              "Draw calls and exact projections, main or reference versus this pass, log scale. "
              + "; ".join(f"{row['label']}: {row['before']:,} to {row['after']:,}" for row in rows))
    svg.text(24, 32, "Per-frame work, before → after", "title ink")
    svg.text(24, 52, "Main or the reference path versus this pass · log scale · counts per frame",
             "subtitle ink2")
    svg.legend(736, 32, [("main / reference", "main", "dot"), ("this pass", "pass", "dot")])
    left, right = 330, 720
    decades = 4

    def x(value):
        return left + (right - left) * math.log10(max(value, 1)) / decades

    plot_top, plot_bottom = 84, 84 + 44 * len(rows) + 8
    for exponent in range(decades + 1):
        tick = 10 ** exponent
        svg.line(x(tick), plot_top, x(tick), plot_bottom, "grid")
        svg.text(x(tick), plot_bottom + 18, number(tick), "tick muted", "middle")
    for index, row in enumerate(rows):
        y = 108 + 44 * index
        svg.text(left - 24, y + 4, row["label"], "label ink", "end")
        svg.line(x(row["after"]), y, x(row["before"]), y, "link")
        svg.dot(x(row["before"]), y, "main")
        svg.dot(x(row["after"]), y, "pass")
        svg.text(x(row["before"]) + 10, y + 4, number(row["before"]), "value ink2")
        svg.text(x(row["after"]) - 10, y + 4, number(row["after"]), "value ink2", "end")
    svg.text(24, height - 14, "Sources: scale_shadow_benchmark and the exact-pixel tests in "
             "crates/bozzard-render/tests/submission.rs.", "subtitle muted")
    return svg.finish()


def cpu_breakdown(data):
    stages = data["cpu_stages_140k"]["stages"]
    total = sum(stage["ms"] for stage in stages)
    height = 100 + 30 * len(stages) + 50
    svg = Svg(760, height, "Remaining renderer CPU at 140k surfaces",
              f"Per-frame renderer CPU by stage, this pass, about {total:.0f} ms in total. "
              + "; ".join(f"{s['label']} {s['ms']:.1f} ms" for s in stages))
    svg.text(24, 32, "Where the remaining renderer CPU goes at 140k surfaces", "title ink")
    svg.text(24, 52, f"This pass · ~{total:.0f} ms per frame, spread over exact O(N) passes "
             "across every surface", "subtitle ink2")
    left, right, ceiling = 210, 700, 25
    top = 76
    bottom = top + 30 * len(stages) + 4
    for tick in range(0, ceiling + 1, 5):
        tx = left + (right - left) * tick / ceiling
        svg.line(tx, top, tx, bottom, "grid" if tick else "axis")
        svg.text(tx, bottom + 18, f"{tick} ms", "tick muted", "middle")
    for index, stage in enumerate(stages):
        y = top + 8 + 30 * index
        end = left + (right - left) * stage["ms"] / ceiling
        svg.text(left - 12, y + 12, stage["label"], "label ink", "end")
        svg.bar(left, y, end, 16, "pass")
        svg.text(end + 8, y + 12, f"{stage['ms']:.1f}", "value ink2")
    svg.text(24, height - 14, "Making these passes change-driven (dirty lists from extraction to the "
             "renderer) is the next step.", "subtitle muted")
    return svg.finish()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    data = json.loads(DATA.read_text())
    charts = {"frame-time.svg": frame_time(data), "work-reduction.svg": work_reduction(data),
              "cpu-breakdown.svg": cpu_breakdown(data)}
    stale = []
    for name, content in charts.items():
        path = OUT / name
        if args.check:
            if not path.exists() or path.read_text() != content:
                stale.append(name)
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        print(path)
    if stale:
        print("stale: " + ", ".join(stale), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
