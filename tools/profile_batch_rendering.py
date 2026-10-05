#!/usr/bin/env python3
"""Compare saved release test executables without rebuilding between timed runs.

Uses the existing deterministic Earth Factory fixtures, alternating executable
order between independent process pairs. Exact RGB captures are checked across
versions; each fixture separately asserts exact RGBA against its reference path.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess


PROFILES = {
    "graph": (
        "profile_earth_factory_graph_instancing",
        "BOZZARD_GRAPH_INSTANCING_OUTPUT",
    ),
    "full": (
        "profile_earth_factory_batch_planning",
        "BOZZARD_RETAINED_RENDER_OUTPUT",
    ),
}
WORKLOADS = ("active_factory", "frozen_factory", "camera_only_factory")
METRICS = (
    "total_cpu", "extraction", "renderer_cpu", "renderer_prepare",
    "batch_planning", "renderer_encode", "renderer_submit", "synchronized",
    "gpu_pass_sum",
)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def profile(binary, destination, kind):
    destination.mkdir(parents=True, exist_ok=True)
    name, output_variable = PROFILES[kind]
    environment = os.environ.copy()
    environment[output_variable] = str(destination.resolve())
    command = [str(binary.resolve()), name, "--ignored", "--exact",
               "--nocapture", "--test-threads=1"]
    with (destination / "test.log").open("w") as log:
        process = subprocess.run(command, env=environment, stdout=log,
                                 stderr=subprocess.STDOUT, check=False)
    if process.returncode:
        raise RuntimeError(f"Profile failed; see {destination / 'test.log'}")
    reports = {}
    for path in destination.glob("*.json"):
        report = json.loads(path.read_text())
        if "workload" in report:
            reports[report["workload"]] = report
    if set(reports) != set(WORKLOADS):
        raise RuntimeError(f"Missing workload reports in {destination}")
    for report in reports.values():
        assert report["build"] == "release"
        assert report["exact_pixels"] and report["scene_and_checkpoint_preserved"]
        assert report["warmup_frames"] == 12 and report["measured_frames"] == 60
    return reports


def compare_captures(before, after, max_channel_delta=0, max_differing_pixels=0):
    captures = []
    before_files = {path.name: path for path in before.glob("*.ppm")}
    after_files = {path.name: path for path in after.glob("*.ppm")}
    if before_files.keys() != after_files.keys() or len(before_files) != 24:
        raise RuntimeError("Expected the same 24 captures from each executable")
    for name, path in sorted(before_files.items()):
        original, candidate = path.read_bytes(), after_files[name].read_bytes()
        equal = original == candidate
        differing_pixels, channel_delta = 0, 0
        if not equal:
            original_parts, candidate_parts = original.split(b"\n", 3), candidate.split(b"\n", 3)
            original_header, candidate_header = original_parts[:3], candidate_parts[:3]
            if len(original_parts) != 4 or len(candidate_parts) != 4 or original_header != candidate_header or original_header[0] != b"P6" or original_header[2] != b"255":
                raise RuntimeError(f"Capture format or dimensions differ: {name}")
            original_pixels, candidate_pixels = original_parts[3], candidate_parts[3]
            width, height = map(int, original_header[1].split())
            if len(original_pixels) != width * height * 3 or len(candidate_pixels) != len(original_pixels):
                raise RuntimeError(f"Invalid capture payload: {name}")
            for offset in range(0, len(original_pixels), 3):
                delta = max(abs(original_pixels[offset + i] - candidate_pixels[offset + i]) for i in range(3))
                differing_pixels += delta != 0
                channel_delta = max(channel_delta, delta)
        accepted = channel_delta <= max_channel_delta and differing_pixels <= max_differing_pixels
        captures.append({"file": name, "exact_rgb": equal,
                         "differing_rgb_pixels": differing_pixels,
                         "max_channel_delta_8bit": channel_delta,
                         "within_recorded_tolerance": accepted,
                         "baseline_sha256": sha256(path),
                         "optimized_sha256": sha256(after_files[name])})
    if not all(capture["within_recorded_tolerance"] for capture in captures):
        raise RuntimeError(f"Cross-version captures exceed the recorded tolerance in {before} and {after}")
    return captures


def result(report):
    # Mode 1 is the default production path in both existing paired fixtures.
    return report["results"][1]


def aggregate(pairs, kind):
    comparisons = {}
    for workload in WORKLOADS:
        versions = {
            version: [result(pair[version][kind][workload]) for pair in pairs]
            for version in ("baseline", "optimized")
        }
        metrics = {}
        for metric in METRICS:
            if metric not in versions["baseline"][0]:
                continue
            measurements = {
                version: [sample[metric]["median_ms"] for sample in samples]
                for version, samples in versions.items()
            }
            medians = {version: statistics.median(values)
                       for version, values in measurements.items()
                       if all(value is not None for value in values)}
            if len(medians) != 2:
                continue
            metrics[metric] = {
                "run_medians_ms": measurements,
                "median_of_run_medians_ms": medians,
                "reduction_percent": 100 * (1 - medians["optimized"] / medians["baseline"])
                if medians["baseline"] else None,
                "sample_counts": {
                    version: [sample[metric]["samples"] for sample in samples]
                    for version, samples in versions.items()
                },
            }
        comparisons[workload] = {
            "metrics": metrics,
            "last_frames": {
                version: [sample["last_measured_frame"] for sample in samples]
                for version, samples in versions.items()
            },
        }
    return comparisons


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--optimized", required=True, type=Path)
    parser.add_argument("--baseline-commit", required=True)
    parser.add_argument("--optimized-commit", required=True)
    parser.add_argument("--output", type=Path,
                        default=Path("work/batch-rendering-optimizations/comparison"))
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--profile", choices=("graph", "full", "both"), default="both")
    parser.add_argument("--max-channel-delta", type=int, default=0)
    parser.add_argument("--max-differing-pixels", type=int, default=0)
    arguments = parser.parse_args()
    if arguments.runs < 1:
        parser.error("--runs must be positive")
    if not 0 <= arguments.max_channel_delta <= 255 or arguments.max_differing_pixels < 0:
        parser.error("Capture tolerance requires a channel delta in 0..255 and nonnegative pixel count")
    kinds = tuple(PROFILES) if arguments.profile == "both" else (arguments.profile,)
    binaries = {"baseline": arguments.baseline, "optimized": arguments.optimized}
    pairs = []
    captures = []
    for run in range(arguments.runs):
        order = ("baseline", "optimized") if run % 2 == 0 else ("optimized", "baseline")
        pair = {version: {} for version in binaries}
        for kind in kinds:
            for version in order:
                destination = arguments.output / f"run-{run + 1}" / kind / version
                print(f"Run {run + 1}/{arguments.runs}: {kind} {version}", flush=True)
                pair[version][kind] = profile(binaries[version], destination, kind)
            before = arguments.output / f"run-{run + 1}" / kind / "baseline"
            after = arguments.output / f"run-{run + 1}" / kind / "optimized"
            captures.append({"run": run + 1, "profile": kind,
                             "captures": compare_captures(before, after, arguments.max_channel_delta, arguments.max_differing_pixels)})
        pairs.append(pair)
        # Preserve completed measurements even if a later process fails.
        summary = {
            "baseline_commit": arguments.baseline_commit,
            "optimized_commit": arguments.optimized_commit,
            "binary_sha256": {version: sha256(path) for version, path in binaries.items()},
            "completed_process_pairs": len(pairs),
            "alternating_executable_order": True,
            "cross_version_rgb_tolerance": {
                "max_channel_delta_8bit": arguments.max_channel_delta,
                "max_differing_pixels_per_capture": arguments.max_differing_pixels,
            },
            "comparisons": {kind: aggregate(pairs, kind) for kind in kinds},
            "cross_version_captures": captures,
            "reports": pairs,
            "limits": [
                "Renderer CPU and full extraction/render CPU are different metrics.",
                "GPU timestamp sample counts can be sparse; no GPU speedup is established by CPU reductions.",
                "Cross-version PPM comparison checks RGB; paired fixtures independently assert RGBA.",
                "The fixture excludes simulation time, GUI, presentation, and interactive FPS.",
                "Scope-specific work-count fixtures establish their own reductions, not factory timing gains.",
            ],
        }
        arguments.output.mkdir(parents=True, exist_ok=True)
        (arguments.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(f"Proof saved to {arguments.output / 'summary.json'}", flush=True)


if __name__ == "__main__":
    main()
