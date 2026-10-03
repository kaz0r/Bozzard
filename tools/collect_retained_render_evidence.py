#!/usr/bin/env python3
"""Validate and summarize three paired retained-render profiles.

Requires Pillow and NumPy. Each run directory must contain all three workload
JSON reports and both unmodified PPM captures at four camera headings. Missing
inputs, mismatched metadata, or changed pixels reject the complete evidence set
before any output is written. Outputs preserve every raw report and counter,
and arrange one validated capture pair at its original pixel dimensions.
"""

import argparse
import hashlib
import io
import json
import math
import statistics
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont


WORKLOADS = ("active_factory", "frozen_factory", "camera_only_factory")
MODES = {
    "owned_reference_full_path": "Owned/reference path",
    "retained_full_path": "Retained path",
}
STAGES = (
    "extraction",
    "frame_retirement",
    "adapter",
    "asset_scan",
    "material_prepare",
    "surface_prepare",
    "total_cpu",
    "renderer_cpu",
    "renderer_prepare",
    "renderer_encode",
    "renderer_submit",
    "synchronized",
    "gpu_pass_sum",
)
ADAPTER_STAGES = {"adapter", "asset_scan", "material_prepare"}
SUBMITTED_COUNTS = (
    "scene_items",
    "surfaces",
    "visible_items",
    "visible_surfaces",
    "color_draws",
    "color_triangles",
    "shadow_draws",
    "shadow_triangles",
)
HEADINGS = (0, 90, 180, 270)
COLORS = ("#a85b3a", "#157b96")
INK = "#17303e"
MUTED = "#506775"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_distribution(value, context, expected_samples=None):
    require(isinstance(value, dict), f"{context}: missing timing distribution")
    samples = value.get("samples")
    require(type(samples) is int and samples >= 0, f"{context}: invalid sample count")
    if expected_samples is not None:
        require(
            samples == expected_samples,
            f"{context}: expected {expected_samples} samples",
        )
    if samples == 0:
        return
    timings = [value.get(key) for key in ("median_ms", "p95_ms", "p99_ms")]
    require(
        all(type(v) in (int, float) and math.isfinite(v) and v >= 0 for v in timings),
        f"{context}: timings must be finite and nonnegative",
    )
    require(timings == sorted(timings), f"{context}: percentiles are out of order")


def validate_report(report, path, workload):
    require(isinstance(report, dict), f"{path}: expected a JSON object")
    require(report.get("workload") == workload, f"{path}: wrong workload")
    require(report.get("build") == "release", f"{path}: expected a release build")
    for flag in (
        "alternating_mode_order",
        "exact_pixels",
        "submitted_counts_equal",
        "scene_and_checkpoint_preserved",
    ):
        require(report.get(flag) is True, f"{path}: {flag} was not confirmed")
    metadata = {
        key: report.get(key)
        for key in (
            "adapter", "backend", "viewport", "warmup_frames", "measured_frames"
        )
    }
    require(metadata["warmup_frames"] == 12, f"{path}: expected 12 warm-up frames")
    require(metadata["measured_frames"] == 60, f"{path}: expected 60 measured frames")
    require(
        isinstance(metadata["viewport"], list)
        and len(metadata["viewport"]) == 2
        and all(type(v) is int and v > 0 for v in metadata["viewport"]),
        f"{path}: invalid viewport",
    )
    require(
        all(
            isinstance(metadata[key], str) and metadata[key]
            for key in ("adapter", "backend")
        ),
        f"{path}: missing GPU identity",
    )
    results = report.get("results")
    require(
        isinstance(results, list)
        and len(results) == 2
        and all(isinstance(result, dict) for result in results),
        f"{path}: expected two paired mode reports",
    )
    require(
        {result.get("mode") for result in results} == set(MODES),
        f"{path}: unexpected or duplicate modes",
    )
    for result in results:
        mode = result["mode"]
        for stage in STAGES:
            value = result.get(stage)
            context = f"{path}: {mode}.{stage}"
            if stage in ADAPTER_STAGES and mode == "owned_reference_full_path":
                require(value is None, f"{context}: reference timing must be unavailable")
            else:
                expected_samples = None if stage == "gpu_pass_sum" else 60
                validate_distribution(value, context, expected_samples)
        counters = result.get("last_measured_frame")
        require(isinstance(counters, dict), f"{path}: missing work counters")
        require(
            isinstance(counters.get("renderer"), dict),
            f"{path}: missing renderer counters",
        )
        if mode == "retained_full_path":
            require(
                isinstance(counters.get("adapter"), dict),
                f"{path}: missing retained adapter counters",
            )
    left, right = [result["last_measured_frame"]["renderer"] for result in results]
    for key in SUBMITTED_COUNTS:
        require(key in left and key in right, f"{path}: missing submitted count {key}")
        require(left[key] == right[key], f"{path}: submitted count differs: {key}")
    return metadata


def load_reports(source):
    runs, sources = [], []
    metadata = None
    for run in range(1, 4):
        workloads = {}
        for workload in WORKLOADS:
            path = source / f"run{run}" / f"{workload}.json"
            raw = path.read_bytes()
            report = json.loads(raw)
            current = validate_report(report, path, workload)
            if metadata is None:
                metadata = current
            require(current == metadata, f"{path}: run metadata differs")
            # Preserve the complete report, including unknown/new diagnostic fields.
            workloads[workload] = report
            sources.append(
                {
                    "path": str(path.relative_to(source)),
                    "sha256": hashlib.sha256(raw).hexdigest(),
                }
            )
        runs.append({"run": run, "workloads": workloads})
    return metadata, runs, sources


def read_capture(path, viewport):
    raw = path.read_bytes()
    with Image.open(io.BytesIO(raw)) as image:
        require(image.format == "PPM", f"{path}: expected a PPM capture")
        require(image.mode == "RGB", f"{path}: expected raw RGB pixels")
        require(image.size == tuple(viewport), f"{path}: capture dimensions differ")
        capture = image.copy()
    return capture, hashlib.sha256(raw).hexdigest()


def validate_captures(source, viewport):
    pairs, selected = [], None
    for run in range(1, 4):
        for workload in WORKLOADS:
            for heading in HEADINGS:
                paths = [
                    source / f"run{run}" / f"{workload}-heading{heading}-{mode}.ppm"
                    for mode in ("reference", "retained")
                ]
                captures = [read_capture(path, viewport) for path in paths]
                pixels = [np.asarray(capture) for capture, _ in captures]
                difference = np.abs(
                    pixels[0].astype(np.int16) - pixels[1].astype(np.int16)
                )
                changed = int(np.count_nonzero(np.any(difference != 0, axis=2)))
                maximum = int(difference.max())
                require(
                    changed == 0,
                    f"{paths[0]}: {changed:,} changed pixels; max RGB delta {maximum}",
                )
                if run == 1 and workload == "active_factory" and heading == 0:
                    selected = [capture for capture, _ in captures]
                pairs.append(
                    {
                        "run": run,
                        "workload": workload,
                        "heading_degrees": heading,
                        "pixels": viewport[0] * viewport[1],
                        "changed_rgb_pixels": changed,
                        "maximum_rgb_channel_delta": maximum,
                        "captures": [
                            {"path": str(path.relative_to(source)), "sha256": digest}
                            for path, (_, digest) in zip(paths, captures)
                        ],
                    }
                )
    return pairs, selected


def aggregate(runs):
    summary = {}
    for workload in WORKLOADS:
        summary[workload] = {}
        for mode in MODES:
            reports = [
                next(
                    result
                    for result in run["workloads"][workload]["results"]
                    if result["mode"] == mode
                )
                for run in runs
            ]
            stages = {}
            for stage in STAGES:
                distributions = [report[stage] for report in reports]
                available = [
                    value for value in distributions if value and value["samples"]
                ]
                result = {
                    "available_runs": len(available),
                    "run_samples": [
                        value["samples"] if value else None for value in distributions
                    ],
                }
                for quantile in ("median", "p95", "p99"):
                    numbers = [
                        value[f"{quantile}_ms"] if value and value["samples"] else None
                        for value in distributions
                    ]
                    result[f"run_{quantile}_ms"] = numbers
                    # A summary requires measurements from all three independent runs.
                    result[f"median_of_run_{quantile}s_ms"] = (
                        statistics.median(numbers) if len(available) == 3 else None
                    )
                stages[stage] = result
            summary[workload][mode] = {
                "stages": stages,
                "last_measured_frame_by_run": [
                    report["last_measured_frame"] for report in reports
                ],
            }
    return summary


def font(size):
    for path in (
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ):
        if Path(path).is_file():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default(size=size)


def capture_figure(captures, metadata, pair_count):
    width, height = metadata["viewport"]
    margin, gap, header, footer = 24, 24, 152, 190
    image = Image.new(
        "RGB", (width * 2 + margin * 2 + gap, height + header + footer), "#f7f9fb"
    )
    draw = ImageDraw.Draw(image)
    draw.text(
        (margin, 20), "Factory native captures: unchanged pixels",
        font=font(36), fill=INK,
    )
    draw.text(
        (margin, 72),
        f"{metadata['adapter']} · {metadata['backend']} · "
        "run 1 · active factory · heading 0°",
        font=font(25), fill=MUTED,
    )
    for index, (capture, label) in enumerate(zip(captures, MODES.values())):
        x = margin + index * (width + gap)
        draw.text((x, 113), label, font=font(27), fill=COLORS[index])
        # Paste validated native pixels unchanged: no resizing, cropping, or filtering.
        image.paste(capture, (x, header))
    draw.text(
        (margin, height + header + 21),
        f"All {pair_count} paired captures: 0 changed RGB pixels; "
        "maximum channel difference 0.",
        font=font(27), fill=INK,
    )
    draw.text(
        (margin, height + header + 66),
        "Three runs × three workloads × four headings. "
        f"Each displayed capture is {width} × {height}, pasted at native size.",
        font=font(24), fill=MUTED,
    )
    draw.text(
        (margin, height + header + 110),
        "Captures are outside timings. Measured path: extraction/conversion + "
        "draw + retirement; both paths share dense transforms and the adapter.",
        font=font(22), fill=MUTED,
    )
    draw.text(
        (margin, height + header + 150),
        "Timings exclude simulation, widget construction, streaming/compute "
        "bridge, swapchain/presentation, and editor panels.",
        font=font(22), fill=MUTED,
    )
    return image


def evidence_report(metadata, runs, sources, pairs):
    return {
        "schema_version": 1,
        "comparison": (
            "Same-branch owned/reference path versus retained path; "
            "not a main-branch before/after comparison."
        ),
        "shared_work": (
            "Both paths use the branch's dense transforms and common adapter "
            "conversion implementation."
        ),
        "method": {
            "runs": 3,
            "timing_summary": (
                "Median of the three independent run medians; median of the "
                "three independent run p95 and p99 values."
            ),
            "unavailable_timings": (
                "Unavailable stages remain null. A timing summary requires "
                "measurements in all three runs."
            ),
            "total_cpu": (
                "Scene extraction and adapter conversion + renderer CPU + "
                "frame retirement; GPU wait excluded."
            ),
            "synchronized": (
                "Extraction + draw + GPU wait + frame retirement; "
                "profiling-result polling excluded."
            ),
            "measured_path_scope": (
                "Extraction/conversion + draw + frame retirement. Excludes "
                "simulation, widget construction, streaming/compute bridge, "
                "swapchain/presentation, and editor panels."
            ),
            "counter_scope": (
                "Work counters are from the last measured frame of each run, "
                "not aggregated across frames."
            ),
            "surface_preparation_bytes_scope": (
                "Retained renderer Vec capacities only; excludes owned "
                "key-string heaps, shared Arc storage, and GPU data. "
                "No allocation-count claim is made."
            ),
            "pixel_scope": (
                "Raw PPM RGB pixels are compared exactly. The native acceptance/"
                "profile harness also compares complete RGBA captures."
            ),
        },
        "metadata": metadata,
        "summary": aggregate(runs),
        "runs": runs,
        "report_sources": sources,
        "pixel_comparisons": pairs,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="Folder containing run1, run2, and run3")
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        metadata, runs, sources = load_reports(args.source)
        pairs, captures = validate_captures(args.source, metadata["viewport"])
        composite = capture_figure(captures, metadata, len(pairs))
        evidence = evidence_report(metadata, runs, sources, pairs)
        serialized = json.dumps(evidence, indent=2, allow_nan=False) + "\n"
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"Evidence rejected: {error}\nNo output was written.\n")
    measurements = args.repo_root / "docs/measurements/retained-render-scenes"
    images = args.repo_root / "docs/images"
    measurements.mkdir(parents=True, exist_ok=True)
    images.mkdir(parents=True, exist_ok=True)
    report = measurements / "benchmarks.json"
    report.write_text(serialized)
    composite.save(images / "retained-render-native.png")
    print(f"retained_render_evidence={report}")


if __name__ == "__main__":
    main()
