#!/usr/bin/env python3
"""Plot measured native-frame motion and arrange unmodified GPU capture crops.

Requires Matplotlib and Pillow. Input is benchmark_interpolation's output folder;
--captures optionally selects capture_interpolation's PPM folder.
"""

import argparse
import csv
import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from PIL import Image, ImageDraw, ImageFont


def plot_motion(source, destination):
    with (source / "motion.csv").open(newline="") as file:
        rows = list(csv.DictReader(file))
    figure, axes = plt.subplots(1, 3, figsize=(13.2, 4.2), sharey=True)
    for hz, axis in zip([60, 120, 144], axes):
        for interpolated, color, label in [
            ("false", "#a64a29", "Exact tick poses"),
            ("true", "#147c96", "Interpolated poses"),
        ]:
            samples = [
                row
                for row in rows
                if row["hz"] == str(hz)
                and row["threaded"] == "true"
                and row["interpolated"] == interpolated
            ]
            steps = [
                (float(b["time_s"]), float(b["shown_x"]) - float(a["shown_x"]))
                for a, b in zip(samples, samples[1:])
                if 0.75 <= float(b["time_s"]) <= 0.88
            ]
            axis.plot(
                [time * 1000 for time, _ in steps],
                [delta for _, delta in steps],
                ".-",
                color=color,
                label=label,
                linewidth=1.5,
                markersize=6,
            )
        axis.axhline(3 / hz, color="#5b6770", linestyle="--", linewidth=1, label="Expected step")
        axis.set_title(f"{hz} Hz frame clock")
        axis.set_xlabel("Accumulated frame time (ms)")
        axis.set_ylim(-0.004, 0.057)
        axis.grid(alpha=0.18)
        axis.spines[["top", "right"]].set_visible(False)
    axes[0].set_ylabel("Presented displacement (world units / frame)")
    figure.suptitle("Fixed 60 Hz simulation: motion between native frames", fontsize=15)
    handles, labels = axes[0].get_legend_handles_labels()
    figure.legend(handles, labels, loc="lower center", ncol=3, frameon=False)
    figure.tight_layout(rect=(0, 0.1, 1, 0.94))
    figure.savefig(destination / "motion-steps.png", dpi=150)
    plt.close(figure)


def capture_strip(source, destination):
    images = {}
    for interpolated in [False, True]:
        for frame in [143, 144, 145]:
            path = source / f"hz144-interpolation{str(interpolated).lower()}-threadedtrue-frame{frame}.ppm"
            with Image.open(path) as image:
                images[interpolated, frame] = image.convert("RGB")
    # A fixed crop includes the mover, its parented orange cube, and checker floor.
    # Pixel data is only cropped and scaled uniformly; the original image is saved too.
    crop = (300, 160, 620, 420)
    width, height = 480, 390
    strip = Image.new("RGB", (width * 3, 100 + 2 * (height + 70)), "#f7f9fb")
    draw = ImageDraw.Draw(strip)
    title = ImageFont.load_default(size=26)
    caption = ImageFont.load_default(size=19)
    detail = ImageFont.load_default(size=17)
    draw.text((20, 14), "144 Hz synthetic clock: three consecutive editor Play frames", font=title, fill="#172d3a")
    draw.text((20, 51), "Native GPU captures; serial and worker pixels match exactly", font=caption, fill="#425867")
    changes = {}
    for row, interpolated in enumerate([False, True]):
        y = 100 + row * (height + 70)
        for column, frame in enumerate([143, 144, 145]):
            x = column * width
            image = images[interpolated, frame].crop(crop).resize((width, height))
            strip.paste(image, (x, y + 60))
            mode = "Interpolated" if interpolated else "Exact tick"
            draw.text((x + 12, y + 5), f"{mode} · frame {frame}", font=caption, fill="#172d3a")
            if column > 0:
                current = np.asarray(images[interpolated, frame])
                previous = np.asarray(images[interpolated, frame - 1])
                count = int(np.any(current != previous, axis=2).sum())
                changes[f"{mode.lower()}_{frame - 1}_to_{frame}"] = count
                draw.text((x + 12, y + 31), f"Changed pixels: {count:,} / 518,400", font=detail, fill="#425867")
    strip.save(destination / "native-frames.png")
    images[True, 144].save(destination / "native-frame.png")
    (destination / "pixel-changes.json").write_text(json.dumps(changes, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--captures", type=Path)
    parser.add_argument("--out", type=Path, default=Path("work/interpolation/proof"))
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    plot_motion(args.source, args.out)
    if args.captures:
        capture_strip(args.captures, args.out)
    print(f"interpolation_proof={args.out}")


if __name__ == "__main__":
    main()
