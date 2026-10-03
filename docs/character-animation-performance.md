# Character animation measurements

Measured on 2026-10-03 on an Apple M2 Pro running macOS 26.6, with Rust 1.95.0 and the workspace release profile (thin LTO, one codegen unit). The baseline is main at `bf8d25d712050b66f130cb31dcc9283669b64f50`. Both executables use the same basic workload and run on the same machine, alternating baseline/changed processes three times after builds and graphics checks completed.

Each case has 64 joints per actor, two one-second translation clips, a 1D blend parameter, 120 warmup ticks and 600 measured animation ticks at 60 Hz. Reported medians are the median of the three process medians. This measures CPU `step_animations`, including its world/controller work; it excludes simulation systems, GPU execution and rendering. The raw process results and environment are in [character-animation.json](benchmarks/character-animation.json).

| Actors | Playback | Main median, µs | Changed median, µs | Reduction |
| ---: | --- | ---: | ---: | ---: |
| 1 | Playing | 4.667 | 4.041 | 13.4% |
| 1 | Paused | 0.375 | 0.292 | 22.1% |
| 32 | Playing | 157.417 | 131.167 | 16.7% |
| 32 | Paused | 8.667 | 6.583 | 24.0% |
| 128 | Playing | 705.125 | 599.875 | 14.9% |
| 128 | Paused | 38.583 | 30.167 | 21.8% |

The single paused actor is close to timer granularity; its absolute difference is 0.083 µs. Desktop scheduling and power state affect all measurements. An initial baseline capture made during development gave an unusually high single-actor result; it is excluded from this comparison. The raw file also preserves an earlier comparison against the preceding main commit. Its timings are from a different desktop session, so they do not isolate the later spatial-setup refinement. These results describe this workload and machine, not a general FPS improvement.

## Cost of the added features

The advanced workload uses a 2D blend with three samples, one additive layer masked to the latter half of the skeleton, and two analytic three-bone IK chains targeting world points. It includes the runtime's spatial setup. It does not include terrain raycasts, root-motion extraction, motion warping or a physics step. Retargeting is baked offline and adds no special playback operation.

| Actors | Median CPU tick, ms | Median of process p95 values, ms |
| ---: | ---: | ---: |
| 1 | 0.0130 | 0.0229 |
| 32 | 0.4750 | 0.6210 |
| 128 | 2.0506 | 2.8427 |

## Changes behind the measurements

- Playback borrows the immutable Animator instead of cloning controller maps every tick. Pose sampling, global transforms, blend scratch and palette output reuse buffers after warmup. A palette still copies when another snapshot holds it; readers retain a consistent pose.
- Triangulation, layer masks, additive reference poses and rest transforms are prepared once for the active controller. Checkpoint clones omit this prepared data and reconstruct it lazily. At most three clips contribute to a directional blend.
- World-point IK skips unrelated scene transforms and collision geometry. Object goals prepare transforms; ground probes additionally prepare collision geometry and support transforms.
- Basic playback skips actor matrix construction when no root motion or IK needs it. Paused poses return before blend lookup or clock work when their controller and parameters are unchanged. Grounded IK continues updating on a paused actor so its contacts can follow a moving support.
- Retargeting reuses source/target pose and rotation buffers for every sampled frame. It validates frame/key budgets before allocating tracks and checks cancellation between frames. Playback consumes the resulting ordinary clips.
- Imported and retargeted clips remove static rest channels and collapse constant step/linear channels. Animated quaternion axes remain in lockstep, and cubic tracks retain their tangents. The taller demo character's ten clips go from 78,200 scalar keys before compaction to 10,259, an 86.9% reduction. Tests compare sampled poses before and after compaction at 101 times, including animated rotations, constant non-rest scales, negative quaternion signs and cubic tangents.
- The Blender exporter publishes one skin with eighteen bindings per human. The editable Blender source keeps separate mesh parts; joining them for GLB export avoids a duplicated eighteen-joint skin for every part.

Reproduce the current measurements with:

```sh
cargo run --release -p bozzard-scene --example animation_bench
```

For a baseline comparison, build the basic benchmark against the pinned main commit in a separate checkout, omitting the advanced workload that the older API cannot represent. Warm both executables and alternate process runs after compilation has finished. Preserve all cases, including paused actors, rather than selecting only the largest improvement.
