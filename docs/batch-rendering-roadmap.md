# Batch rendering optimization roadmap

This review covers batch construction, instance data, color and shadow submission,
visibility, and the scene and asset work that feeds rendering. It examines main at
`2b43510` on October 5, 2026. This is the historical review and implementation plan.
The resulting changes and validation are recorded in the
[implementation proof report](batch-rendering-optimizations.md) and
[43-item coverage](batch-rendering-coverage.md); source line anchors below refer to the reviewed main revision.

The best next investments are to retain and simplify shadow batches, avoid full
ordering rebuilds, and make unchanged surfaces cheaper to process. For GPU gains,
focus on opaque shadow shaders, useful visibility rejection, and unused shader
inputs and outputs. Larger instance buffers and multi draw are useful scaling
options, but should follow these changes rather than lead the work.

## Evidence and limits

Global batching, 64 instance records, shared frame uniforms, changed range uploads,
shader graph instancing, independent shadow batching, conservative light masks,
retained surface expansion, fitted sun bounds, and static sun depth reuse already
exist. This roadmap extends them; it does not count them as new optimizations.

The latest committed [shader graph measurements](shader-graph-batching.md) report
1,590 visible surfaces in **63 color draws**, including 34 instanced commands and
29 singletons. Nineteen singletons have unique keys and ten are split groups or
tails. On Iris Xe and Vulkan, active renderer CPU was 3.589 ms and the sum of GPU
pass timestamps was 10.456 ms. These measurements exclude extraction, simulation,
presentation, and GUI work. They are evidence that draw count alone is no longer
the whole problem, not timings for every supported machine.

The earlier [retained scene measurements](retained-render-scenes.md) put extraction
at about 0.81 to 0.86 ms and retained surface preparation at about 0.07 to 0.09 ms.
Their raw adapter data puts asset scans around 0.004 ms. Large catalogs and asset
churn justify asset improvements; those small factory scans are not the first
steady frame bottleneck. These reports used different revisions and conditions;
their stage medians must not be added together.

Every proposed benefit below is an inference from the current implementation.
Priorities distinguish broad savings from workload dependent opportunities. No
percentage speedup is promised, and gains from overlapping changes are not additive.

## Fresh native baseline

The existing paired graph instancing benchmark passed on Apple M2 Pro and Metal
at 1280 by 800 in release mode during this review. One process measured 60 frames
per mode and workload after 12 warmup frames. Both modes matched exact captures
at four headings in each workload, color geometry, and scene and checkpoint state.
The [saved report](measurements/batch-rendering-review-2026-10-05.json) retains
stage distributions and last frame counters; full raw profiles and captures are
in ignored `work/batch-rendering-audit/graph/`.

| Current main with graph instancing | Renderer CPU median | Preparation median | Batch planning median | Encoding median | Final color draws |
| --- | ---: | ---: | ---: | ---: | ---: |
| Active factory | 1.540 ms | 1.144 ms | 0.137 ms | 0.034 ms | 63 |
| Frozen factory | 1.076 ms | 0.838 ms | 0.112 ms | 0.024 ms | 63 |
| Moving camera factory | 1.445 ms | 1.133 ms | 0.146 ms | 0.021 ms | 78 |

All three final frames retained their color plan. This orthographic factory test
does not measure the perspective limitation in item 13. Active shadows submitted
only two dynamic draws over cached static depth; grouping and uniform preparation
can still scan much more than those two draws represent.

The final active frame submitted **20 occlusion depth draws**, tested 55 batches,
and reported **zero culled batches or triangles**. The frozen view likewise
reported zero rejection, but reused its depth query instead of drawing the depth
pass again. These counters make item 33 a particularly relevant experiment for
the current open factory view. They do not quantify the speedup from disabling
occlusion; an explicit paired on versus off test is still needed.

Preparation is a larger immediate CPU target than encoding in this run. Only
5 to 13 GPU samples were complete per mode and workload, so the sparse timestamp
results do not establish a GPU improvement or a reliable pass ranking. One run
also does not establish repeatable p99 behavior. Use the broader baseline wave
before assigning numerical savings to any proposed change.

## Priority and implementation order

| Track | Items | Expected value | Order |
| --- | --- | --- | --- |
| Shadow preparation and depth rendering | 1 to 9 | Highest potential when shadows update; local light items require local shadow workloads | Early |
| Batch planning and per surface CPU work | 10 to 20 | High for moving cameras, changing populations, large scenes, and cold frames | Early |
| Extraction and asset dependencies | 21 to 24 | Extraction is broadly relevant; geometry validation and GI are conditional hotspots | Alongside early work |
| Batch compatibility and submission architecture | 25 to 30 | Shader parameter reuse can remove fragmentation; shared arenas enable larger scale | After stable identities |
| Visibility and shader work | 31 to 38 | Largest GPU opportunities in obstructed or shader heavy views | Prototype early and measure |
| Animation and unsupported surface classes | 39 to 42 | High for crowds, sprite games, text, HUD, and transparency; little impact in the recorded factory frame | Dedicated workload tracks |
| Imported geometry | 43 | Vertex work savings multiply across repeated instances and shadow passes | Independent cooking track |

## Shadow preparation and depth rendering

### 1 Retain shadow batch membership

**Finding:** An updating map calls `prepare_shadow_instances`, which rebuilds a
hash map and all caster groups even when only a transform moved.
[Sources](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L554),
[call site](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L2077).

**Implementation:** Retain caster groups independently of map validity. Patch
membership only for geometry, eligibility, or resource changes; update instance
records separately. Reuse group vectors and prepare only groups needed by maps
that will render. Keep static and dynamic membership available to the sun cache.

**Validation:** Sparse movers, offscreen casters, eligibility edits, asset changes,
empty maps, and failed frame retries. Measure grouping and packing CPU separately.

### 2 Give shadows a depth compatibility key

**Finding:** Shadows reuse the color key, including graph hash and PBR host,
although the depth shader evaluates neither.
[Key and grouping](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L126),
[depth shader](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/shadow_cast.wgsl#L1).

**Implementation:** Group by geometry and actual alpha resources, with deformation
and raster constraints. Remove graph and color host identity. After item 4, opaque
groups can also omit color texture identity. Preserve the current graph shadow
contract rather than silently introducing graph evaluation in depth rendering.

**Validation:** Graph and host changes, alpha masks, mirrored and double sided
geometry, all shadow types, and exact depth and color output.

### 3 Use compact shadow instance records

**Finding:** Shadows consume the full 256 byte color record but use only model,
UV scale, opacity, cutoff, and raster flags.
[Layout](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/object.wgsl#L2),
[uses](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/shadow_cast.wgsl#L9).

**Implementation:** Define a separate depth record and binding layout. A mat4 and
two vec4s are a 96 byte candidate; validate WGSL layout before selecting capacity.
If that layout holds, 170 records fit in 16 KiB rather than 64. Track depth revisions
so lighting, normal matrices, previous transforms, and color changes do not upload
shadow data. Update sun, spot, point, singleton, and instanced variants together.

**Validation:** Layout and limits, exact shadows, partial instance ranges, and zero
depth uploads after changes that cannot affect depth.

### 4 Add an opaque shadow pipeline without a fragment shader

**Finding:** Every caster currently samples alpha and runs fragment discard,
including guaranteed opaque geometry.
[Shader](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/shadow_cast.wgsl#L13),
[pipeline](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/shadows.rs#L343).

**Implementation:** Publish reliable coverage metadata for textures and materials.
Prove effective sampled alpha times object opacity exceeds both the cutoff and
the existing `0.00001` discard threshold; omit proven fully rejected casters. Use
a vertex only depth path for proven opaque coverage. Separate hardware culling
variants for mirrored and ordinary single sided meshes; keep double sided and
uncertain alpha semantics. Retain the current fragment path for masked coverage.

**Validation:** Texture alpha reloads, thin meshes, mirrored scales, grazing point
shadows, bias, and GPU time for opaque versus masked depth passes. High GPU potential
depends on how much shadow raster work is truly opaque.

### 5 Reuse local caster visibility and query a spatial index

**Finding:** Each light face scans casters to compare retained state, then tests
them again while drawing. Point lights multiply this work by six faces.
[Comparison](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/local_shadow_maps.rs#L154),
[draw tests](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/shadows.rs#L918).

**Implementation:** Return accepted stable caster IDs with each map change and
consume that set when encoding. Add a conservative world bounds index for light
volume queries. Update old and new bounds so entering and leaving objects dirty
the correct faces, then apply the existing exact predicate to candidates.

**Validation:** Compare caster sets with the brute force path through boundary,
range, cone, transform, removal, and publication changes. This track needs local
shadow lights; the recorded factory fixture does not exercise them.

### 6 Reduce fragmented shadow instance ranges

**Finding:** A batch with visibility holes produces one draw per accepted range.
Static sun masks and narrow light frusta can make batching ineffective.
[Range loop](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/shadows.rs#L964).

**Implementation:** Start with stable spatial ordering and static versus dynamic
partitions within depth groups. Measure a portable per light repack against its
upload cost. After item 28, use compact per pass instance ID lists to reference
shared object records without copying full uniforms. Preserve accepted caster sets.

**Validation:** Alternating accepted members, moving lights, all visible groups,
nonzero ranges, and static cache output. Track range commands, uploads, packing CPU,
and total GPU time together.

### 7 Reuse static depth layers for local shadow maps

**Finding:** The sun has a static depth layer; local maps retain whole maps and
redraw accepted static casters when a dynamic caster changes a face.
[Sun cache](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/sun_cache.rs#L24),
[local maps](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/local_shadow_maps.rs#L200).

**Implementation:** Prototype a static layer per profitable spot or point face.
Keep an immutable static source; refresh a separate working map by copy or equivalent
rendering before loading it and drawing dynamic casters. Drawing into the cached
static source would leave stale depth when a mover departs. Invalidate static
layers on light projection, static membership, alpha resources, resolution, or
depth producing changes. Budget additional textures and evict unused layers.

**Validation:** Cached versus full depth, departed movers, moved lights, static edits, face crossing,
and memory ceilings. Adopt only when saved raster cost exceeds the copy and memory
cost; this is a workload dependent extension, not a guaranteed factory gain.

### 8 Choose static shadow reuse by work saved

**Finding:** The sun cache requires at least 32 static groups. Better batching can
drop below that threshold while leaving expensive static geometry unchanged.
[Heuristic](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/sun_cache.rs#L35).

**Implementation:** Replace the group count rule with a calibrated estimate using
static vertices or triangles, map resolution, dynamic work, and measured copy cost.
Keep a conservative bypass for small scenes and retest after group layout changes.

**Validation:** Few expensive groups, many cheap groups, dense mostly static scenes,
projection changes, and exact cached output. Measure profitability after items 2,
3, and 6 so improved batching does not accidentally disable useful reuse.

### 9 Avoid unchanged local shadow uniform uploads and allocations

**Finding:** Local map updates allocate receiver bytes and rewrite the complete
receiver buffer even when matrices and settings are identical.
[Update](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/local_shadow_maps.rs#L110).

**Implementation:** Retain receiver bytes and matrix scratch storage, compare or
revision track changed rows, and coalesce only those writes. Clear retired slots
when map counts shrink. Keep caster uniforms independent because queued writes
must not overwrite data for another map in the same submission.

**Validation:** Unchanged maps, reorder, count shrink, failed submissions, and write
call counters. This is supporting CPU work, not the largest GPU opportunity.

## Batch planning and per surface CPU work

### 10 Bypass ordering construction when original order already batches well

**Finding:** Even a homogeneous compatibility key builds projections and an
overlap graph. Coincident copies can build quadratic edges unnecessarily.
[Builder](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L250),
[original order certificate](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing/reuse.rs#L63).

**Implementation:** Detect homogeneous opaque sequences and already optimal
consecutive groups, then chunk in original order. Add a cheap expected draw benefit
test before paying for global ordering. Keep transparency in its existing order.

**Validation:** Coincident copies, interleaved materials, capacity tails, and exact
equal depth winners. Measure initial plan latency and edge allocations.

### 11 Bound overlap graph construction

**Finding:** Initial followers can contain `N × (N − 1) / 2`
edges; the later retained certificate budget does not bound this construction.
[Edges](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L270).

**Implementation:** Add a construction budget and safe original or consecutive
order fallback. Improve broad phase axis selection or use a multidimensional
index for remaining candidates. Cap scratch memory and expose fallback reasons.

**Validation:** Dense overlaps, long thin bounds, near plane uncertainty, many keys,
and peak planning memory and p99. Preserve every dependency when using the DAG.

### 12 Query escaped ordering envelopes spatially

**Finding:** Each escaped envelope scans every surface; obsolete dependency pairs
remain until the pair budget forces a rebuild.
[Renewal](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing/reuse.rs#L147).

**Implementation:** Update escaped envelopes in an index, query neighbors, and
retire provably obsolete pairs. Discover simultaneous moves using the complete
new envelope state. Replace repeated linear membership searches with mark arrays.

**Validation:** Many movers, long movement, zoom changes, newly coplanar overlaps,
and randomized equivalence with full rebuilding. Measure candidate checks and
pair storage as well as plan reuse.

### 13 Retain safe plans during perspective movement

**Finding:** Reordered plans use an orthographic certificate; perspective camera
or object movement can force rebuilds despite safely separated geometry.
[Checks](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing/reuse.rs#L80).

**Implementation:** Prototype conservative screen and depth envelopes with
explicit camera validity ranges, or a proven perspective world separation
certificate. Keep original order universally reusable and uncertain projections
on the full rebuild path.

**Validation:** Perspective rotation, translation and zoom, near plane crossings,
large coordinates, quantized equal depth, and exact reference winners. High
potential for perspective views, with substantial correctness complexity.

### 14 Use stable identities throughout retained caches and GPU slots

**Finding:** Adapter and expanded surface caches match rows by position; packed
bindings follow the current batch ordinal. Early insertion, removal, LOD changes,
or culling can shift unrelated rows and slots.
[Adapter](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render-assets/src/frame.rs#L193),
[surfaces](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/preparation.rs#L214),
[packing](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L603).

**Implementation:** Establish scene scoped object generation and part identities.
Retain payload, surface, group, and GPU slots by identity; assemble current draw
order separately. Decouple visibility from persistent compatibility membership,
and patch affected groups. Public zero or duplicate motion IDs need a safe fallback.
Retire stale slots within bounded pools.

**Validation:** Chunk streaming, spawn and despawn, LOD and part count changes,
visibility churn, duplicate identities, asset replacement, transparency ties, and
memory retirement. This is a foundational change for sparse uploads and GPU culling.

### 15 Schedule legal groups for fullness and GPU work

**Finding:** The DAG scheduler selects the ready group with the earliest source
index. Legal alternatives can unlock fuller batches or reduce state changes and
fragment overdraw.
[Scheduler](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L299).

**Implementation:** Score dependency ready groups using bounded lookahead for
members unlocked, batch fullness, resource state, near depth, and spatial locality.
Coalesce compatible adjacent filtered tails where legal. For example, A0, B1, A2
with only B1 before A2 can legally draw B1 then one A batch. Never relax overlap
dependencies to achieve a better score.

**Validation:** DAG fixtures, randomized legal order, coplanar pixel winners,
perspective movement, split singleton reasons, and draws versus opaque GPU time.
Depth prioritization is an experiment, not an assumed universal win.

### 16 Track sparse uniform changes and cache transform derived values

**Finding:** Packed bindings compare every record. Any object source change can
also recompute its inverse normal matrix and determinant, even for a material,
light mask, or previous transform change.
[Packing](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L644),
[uniforms](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L1919).

**Implementation:** Assign internal uniform revisions after exact source comparison.
Cache normal and determinant by current model, and share conservative bounds where
their arithmetic contract matches. Update packed slots by identity and revision.
Measure merging small dirty gaps or using a pooled upload arena to reduce write
calls, preserving existing unchanged range behavior.

**Validation:** Sparse and random edits, all movers, material only and temporal
history changes, mirrored and projective models, exact bytes, and failed retries.
Track comparison cost, write calls, and bytes separately.

### 17 Cache diagnostics and reuse frame scratch storage

**Finding:** Eligibility diagnostics build and hash a peer map each frame. Bounds,
visibility marks, individual flags, and planner scratch arrays also allocate.
[Diagnostics](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing/diagnostics.rs#L37),
[frame arrays](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L1818).

**Implementation:** Cache invariant diagnostic counts, update visibility dependent
counts, and retain scratch buffers with shrink rules. Maintain cheap public stats;
allow expensive detailed diagnostics on demand without changing normal behavior.

**Validation:** Counter parity with the existing collector, allocator measurements,
large unloads, mode changes, and peak retained capacity. A retained vector estimate
does not establish zero allocations.

### 18 Allocate individual GPU bindings only when needed

**Finding:** Every prepared surface gets an individual object buffer and bind group
even when color and shadow draws are entirely instanced.
[Allocation](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L887),
[eager loop](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L1794).

**Implementation:** Separate CPU uniform state from optional singleton resources.
Create or pool a binding after plans establish an individual draw is needed. Use
explicit binding selection instead of eager fallback evaluation. Preserve current
resource and matrix validation even when no singleton binding exists.

**Validation:** Fully instanced cold frames, singleton transitions, texture reloads,
hidden casters, and failures. Expect loading, memory, and resource churn improvements
more than a large warmed frame speedup.

### 19 Stop computing unused opaque depth

**Finding:** Camera changes update projected depth for opaque surfaces although
current opaque sorting uses shader and host; depth consumers are transparent
sorting and particle interleaving.
[Refresh](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/preparation.rs#L175).

**Implementation:** Compute depth only for transparency unless item 15 explicitly
requests an opaque scheduling depth. Refresh correctly when transparency changes.

**Validation:** Transparent ties, particles, alpha classification changes, skin
centers, and exact captures. This is a small, low complexity camera optimization.

### 20 Compile only used pipeline variants and prewarm predictable ones

**Finding:** A new graph compiles all four ordinary host and transparency variants;
used instanced variants are added separately. Stock instancing creates all four
host and output variants on first use.
[Graph compilation](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L825),
[stock variants](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L512).

**Implementation:** Store optional variants under complete flavor keys and compile
only submitted flavors. Prewarm known required variants during loading. Retain
bounded active and idle graph caches and clear error reporting.

**Validation:** First appearance, graph edits, effect toggles, transparent fallback,
cache eviction, and compile counters. Target cold frame hitches, not warm averages.

## Extraction and asset dependencies

### 21 Make transform and topology extraction incremental

**Finding:** Shared payloads are retained, but extraction still validates and scans
transform and topology inputs and constructs output collections.
[Extraction](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-scene/src/lib.rs#L1364),
[membership](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-scene/src/render_extraction.rs#L67).

**Implementation:** Retain topology and global transforms with dependency dirty
propagation, update changed subtrees, and reuse frame collection capacity. Cache
stable entity handles and motion IDs instead of repeating row lookups and hashing. Use
reliable mutation tracking or exact comparisons for public mutations; document
revision and ECS tick alone are insufficient. Reuse the same transform snapshot
across views when world and interpolation inputs match. Once authoritative state
maintenance preserves validation of all transforms, limit render transform work to
drawables, text, sprites, lights, selected cameras, and their ancestor closure.
Simply skipping invalid hidden or logic objects would change the current contract.

**Validation:** Same tick and untracked writes, reparenting, deletion, interpolation,
Edit and Play isolation, frozen frame ownership, and extraction stage p95 and p99.

### 22 Invalidate asset dependencies selectively

**Finding:** Frames scan the catalog and asset publication clears pooled material
records broadly. Residency independently traverses scene requirements and catalog.
[Asset checks](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render-assets/src/frame.rs#L81),
[residency](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render-assets/src/residency.rs#L104).

**Implementation:** Maintain a store publication epoch plus changed asset IDs and
an immutable publication identity that changes on divergent clone mutation. Retain
actual asset data Arcs; numeric epochs alone are insufficient. Track material and
residency dependencies, update required
asset reference counts, and invalidate only affected entries. Keep safe full scan
fallbacks for replacement stores and unsupported mutation routes.

**Validation:** Divergent cloned stores, matching numeric revisions, failed reloads,
generated textures, unrelated publications, streaming, and bounded residency.
Prioritize catalogs or churn where measurements justify it.

### 23 Cache immutable mesh part bounds

**Finding:** The surface existence check calls `mesh_surface`, which calculates
part bounds by folding indexed vertices, even when materials are reused.
[Call](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render-assets/src/frame.rs#L187),
[bounds calculation](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-assets/src/lib.rs#L100).

**Implementation:** Split existence and binding validation from geometry bounds
queries. Publish immutable per part bounds alongside mesh data and invalidate on
actual replacement. Reuse those bounds for extraction, picking, and rendering.

**Validation:** Large repeated `Mesh::Surface` instances, invalid part indices and source signatures,
replacement meshes, and exact bounds. This can be a large hidden CPU hotspot for
surface meshes; whole asset factory instances may benefit little.

### 24 Cache live baked GI freshness dependencies

**Finding:** Runtime GI freshness captures the complete live scene before checking
the bake source, including transform composition and static source serialization.
[Runtime check](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render-assets/src/frame.rs#L563),
[source construction](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-assets/src/gi/mod.rs#L75).

**Implementation:** Retain exact dependencies of the bake source and invalidate on
static geometry and transforms, material, light, sky and environment, volume,
hierarchy, static versus dynamic classification, or relevant asset changes. Avoid unrelated live
scene capture and serialization. Dynamic receiver movement must not incorrectly
invalidate or validate static bake data. Keep conservative checks for untracked
public mutations and failed publication.

**Validation:** Static versus dynamic movement, material and light edits, asset
replacement outside editor counters, cloned stores, and enabled GI pixels. This
track is high potential only in scenes using baked GI.

## Batch compatibility and submission architecture

### 25 Move graph numeric parameters into instance data

**Finding:** Graph float and vector inputs compile into WGSL literals, changing
program hashes and splitting otherwise identical topology into separate batches.
Ordinary material tint, UV, roughness, and metallic overrides already use uniforms.
[Graph literals](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-scene/src/shader_graph.rs#L491),
[source hash](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render-assets/src/shaders.rs#L46).

**Implementation:** Separate graph topology and feature specialization from scalar
and vector values. Assign a bounded graph parameter layout and supply per instance
values. Keep topology and resource changes specialized. Budget the extra data
against portable uniform limits or use item 28 where supported.

**Validation:** Different values sharing topology, graph Time, alpha classification,
texture and keyword differences, parameter edits, and cache keys. Preserve numeric
evaluation order and compare against the literal reference path.

### 26 Share compatible texture and material resources

**Finding:** Exact texture identity remains a color batch boundary. Unique keys
cannot be removed by increasing instance capacity. Material image interning and
GPU aliasing already exist; different logical texture or part IDs can still split
otherwise equivalent resolved resources.
[Compatibility](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L84).

**Implementation:** Extend existing resource canonicalization to missing image
and model map cases, and feed resolved resource and sampler identities into batch
keys. Equivalent model parts also need canonical geometry and shading identities;
identical texture bytes alone cannot merge them. Prototype texture arrays for
compatible dimensions and formats, or carefully padded atlases with per instance
UV transforms. Keep a separate path for incompatible sampler, wrap, mip, color
space, alpha, and graph resource behavior. Avoid assuming universal bindless support.

**Validation:** Mip edges, repeat and clamp behavior, texture replacement, PBR maps,
color space, alpha holes, and exact reference images. This is conditional batching
coverage work; unique geometry still needs a separate draw or shared geometry path.

### 27 Avoid redundant geometry and material binding commands

**Finding:** Pipeline state is cached, but color and shadow draws rebind vertex,
previous vertex, index, and material resources.
[Color](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L1534),
[shadows](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/shadows.rs#L954).

**Implementation:** Track resource identity and slice offsets per pass. Skip
identical bindings; reset state after bundles, sky, particles, and incompatible
layout changes. Add counters for each binding category.

**Validation:** Shared buffers with different offsets, PBR and basic switches,
skinned and static draws, and output parity. Likely modest for 63 color commands,
more useful for fragmented shadow ranges and large populations.

### 28 Add a shared instance arena selected by device capability

**Finding:** The portable path fixes batches at 64 records and creates a separate
buffer and binding for every packed group.
[Limit](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L8),
[device request](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/lib.rs#L132).

**Implementation:** Preserve the baseline path. Select larger supported uniform
bindings or a storage object table with stable records and per pass ID indirection.
Separate object data from textures and materials. Request only supported limits
and features in both player and editor host device creation. Support sparse updates,
bounded growth, and retirement across in flight frames.

**Validation:** Forced baseline and enhanced modes, device limits, large groups,
visibility churn, sparse edits, and memory. Larger capacity alone does not eliminate
the factory's unique keys or ordering constraints.

### 29 Reuse command encoding through render bundles

**Finding:** Retained plans and resources still re encode every batch each frame.
[Color loop](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L2308).

**Implementation:** Prototype bundles keyed by plan, pipeline, buffer, and binding
generations. Keep buffer contents mutable where legal; invalidate on changed
commands or resource identities. Prefer a few useful bundles over per draw bundles.
Reestablish pass state after bundle execution.

**Validation:** Warm and cold encoding, visibility and resource changes, temporal
inputs, and exact output. wgpu bundles replay backend commands; measure CPU savings
without promising reduced hardware draw count. Recorded factory encoding is already
small, so larger command populations are the appropriate scaling workload.

### 30 Use multi draw for compatible shared state

**Finding:** The renderer submits individual direct or indirect commands. Different
current mesh and object bindings prevent simply replacing the loop with multi draw.
[Draw](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L1573).

**Implementation:** After item 28, pack compatible geometry into shared vertex and
index arenas and build argument runs using the same pipeline and material state.
Enable indirect first instance or count capabilities only when supported. Retain
ordinary indirect or direct fallback paths and verify whether multi draw is native
or emulated on each backend.

**Validation:** Geometry offsets, signed base vertex, indirect limits, material
runs, unsupported features, and actual CPU and GPU cost. This is a substantial
architecture track for scale, not an immediate fix for the current factory.

## Visibility and shader work

### 31 Include safe graph surfaces as occlusion candidates

**Finding:** Any graph member excludes its entire batch from occlusion tests,
despite opaque graph instancing now covering most factory graph surfaces.
[Exclusion](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/occlusion.rs#L285).

**Implementation:** Allow candidates with the stock vertex transform and valid
conservative bounds. Keep uncertain or deformed bounds conservative. Candidate
eligibility and occluder eligibility are separate: alpha uncertain graph surfaces
must not become occluders merely because they can be hidden by other geometry.

**Validation:** Graph alpha and discard, Time, coplanar depth, motion, and exact
occlusion disabled captures. Expected benefit depends on actual obstruction.

### 32 Cull instances independently

**Finding:** Occlusion uses the union rectangle and nearest depth of a complete
batch. One visible member can retain many hidden members.
[Union](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/occlusion.rs#L272),
[whole batch test](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/occlusion/cull.wgsl#L24).

**Implementation:** Prototype spatially coherent subgroups first, then per instance
tests and stable order compaction into ID lists with item 28. Reuse world bounds
from retained surface state where conservative equivalence holds. Preserve the
relative order of surviving members for equal depth winners.

**Validation:** Mixed visible and hidden members, sparse distant copies, near plane
crossings, current frame depth, rejected triangles, and compute versus raster cost.
This has high potential in interiors and obstructed repeated geometry.

### 33 Run occlusion only when it saves work

**Finding:** Fixed surface, batch, occluder, and triangle thresholds can pay for
depth and pyramids in open views with little rejection.
[Thresholds](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/occlusion.rs#L7).

**Implementation:** Measure depth, pyramid, cull, and saved raster cost. Score
occluders by coverage and cost, temporarily bypass unprofitable workloads, and
retest periodically or after meaningful view changes. Only pass scheduling adapts;
visibility correctness must never depend on a guessed previous result.

**Validation:** Open factory fields, interiors, rapid transitions, stable views,
and full GPU time. Include both successful rejection and no benefit workloads.

### 34 Specialize auxiliary outputs and previous vertex inputs

**Finding:** Auxiliary output is binary, attaching all three targets when any
consumer needs them. Previous positions are bound and transformed in ordinary
variants even without a temporal consumer.
[Selection](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L1695),
[inputs](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L1554).

**Implementation:** Introduce a bounded normal, motion, and specular target mask.
Specialize vertex layouts, WGSL outputs and varyings, and bindings; remove previous
position fetches and transformations in variants that do not need motion. Include
particles and transparent coverage in consumer analysis.

**Validation:** Every supported effect combination, warm toggles, skinned motion,
graphs, and exact outputs on Metal, Vulkan, and DX12. Backend compilers may already
eliminate some dead work, so GPU measurement is required.

### 35 Skip unused unlit lighting and maps

**Finding:** Basic unlit surfaces calculate GI and sun visibility before mixing
lighting away. Stock PBR computes several maps and TBN before its unlit return.
[Basic](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.wgsl#L81),
[PBR](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/pbr.wgsl#L47).

**Implementation:** Add an early basic unlit path preserving fog, alpha, emissive,
and required auxiliaries. Specialize stock unlit PBR map dependencies. Graphs need
dependency analysis because their base or alpha can depend on other material maps.
The existing lit batch key makes uniform specialization practical.

**Validation:** GI and shadows enabled, alpha textures, fog, graph dependencies,
and auxiliary pixels. Main benefit is scenes with large unlit screen coverage.

### 36 Tighten spotlight masks and skip zero contribution lighting

**Finding:** Surface masks represent spotlights as range spheres, ignoring their
cones. Some fragment paths calculate visibility and BRDF work for contributions
that are provably zero.
[Masks](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/local_lights/culling.rs#L3),
[lighting](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/local_lights.wgsl#L91).

**Implementation:** Add conservative cone versus bounds rejection and include
direction and cone changes in revisions. Reuse geometric light quantities and
guard expensive PCF or BRDF evaluation behind proven zero contribution tests.
Keep original arithmetic and ascending accumulation order where rounding matters.
Evaluate tiled or clustered lists only if masks still leave measured lighting
cost high; the current 32 light limit does not alone justify a new renderer.

**Validation:** Hard and wide cones, tangencies, large coordinates, graph normals,
directional lights, all light slots, and exact full loop output.

### 37 Cull single sided color geometry in hardware

**Finding:** Color pipelines use the default unculling primitive state; PBR can
sample several maps before rejecting a back face in the fragment shader.
[Pipeline](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L390),
[fragment work](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/pbr.wgsl#L47).

**Implementation:** Add culling variants for ordinary and mirrored single sided
geometry, retaining the double sided path. Include raster class in grouping where
required and compile variants lazily. Only select this path where the existing
front facing discard proves equivalent; preserve graph and transparency semantics.

**Validation:** Negative scales, closed and thin meshes, graph front face behavior,
alpha, and exact color and auxiliary output. Measure saved raster work against
extra batch splits; unconditionally splitting every mixed group could lose time.

### 38 Skip absent stock PBR material maps

**Finding:** Lit stock PBR samples neutral metallic and roughness, normal, AO, and
emissive placeholders even when a material has no authored map for them.
[Sampling](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/pbr.wgsl#L47).

**Implementation:** Carry actual map presence metadata and build a bounded set of
lazy stock shader variants substituting the exact neutral values. Combine with
unlit and output variants carefully to control cache size. Leave graphs on their
dependency preserving path unless analysis proves those inputs unused.

**Validation:** Missing maps, reloads that add maps, tangents and normal mapping,
linear and sRGB neutral values, alpha, and exact outputs. Prioritize according to
material coverage and pass timings, not just the count of texture calls.

## Animation and unsupported surface classes

### 39 Cull and share deformation work before batching animated populations

**Finding:** Skinning runs before visibility and scans all actors. Per object
deformed buffers force singleton draws, and model wide bounds are used per part.
[Order](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene.rs#L1757),
[skinning](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/skinning.rs#L242).

**Implementation:** Establish conservative pose bounds before deformation and
dispatch only for actors needed by camera or shadow views. Cache genuinely identical
mesh and palette outputs, considering previous pose history separately. Tighten
part bounds conservatively. Add shared deformed vertex arenas or vertex pulling
with offsets before attempting animated instancing.

**Validation:** Offscreen shadow casters, re entry motion, identical current but
different previous poses, pause, reloads, and crowd GPU timings. Factory evidence
does not establish a benefit for this track.

### 40 Cache rest pose skin palettes

**Finding:** The no animation player fallback reconstructs rest pose palettes and
their allocation each extraction. Playing and paused runtime palettes already
have caching and should not be counted again.
[Fallback](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-scene/src/lib.rs#L1517).

**Implementation:** Retain immutable rest palette and rig signature by actual rig
identity, layout, and rest input. Share them across fallback actors, invalidating
on rig replacement or authoring edits.

**Validation:** Authoring actors without players, shared rigs, rig edits, asset
reloads, and extraction allocations and pose parity.

### 41 Retain CPU text and sprite metadata

**Finding:** World text clones content during extraction and goes through transient
adapter conversion rather than the retained drawable prefix. Sprite extraction
also reconstructs and sorts membership and clones resource identifiers.
[Extraction](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-scene/src/lib.rs#L1456),
[adapter](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render-assets/src/frame.rs#L486),
[sprite membership](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-scene/src/middleware/sprite/mod.rs#L744).

**Implementation:** Cache immutable text inputs and converted payloads by exact
content, font, layout, and atlas identity. Refresh transforms and colors separately.
Cache sprite membership until structural edits and reuse immutable resource metadata.
Retain existing GPU text and sprite geometry caching; this item targets additional
CPU work.

**Validation:** Changing text, font and atlas changes, sprite membership and
opacity edits, clipping, scale, frozen frames, and CPU allocations in text and
sprite heavy scenes.

### 42 Batch sprites text HUD and ordered transparency

**Finding:** These surface classes are excluded from opaque instance grouping;
HUD submits an item at a time. The recorded factory frame has none of these
singleton reasons, so this is coverage work for other products and scenes.
[Exclusions](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/instancing.rs#L94),
[HUD](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-render/src/scene/hud.rs#L188).

**Implementation:** Use shared quad instances for compatible sprites; concatenate
adjacent text glyph runs sharing atlas and state; merge consecutive HUD runs with
the same texture and scissor. Start transparent instancing with consecutive
compatible runs in the existing back to front order. Preserve particle interleave
boundaries. Never apply global opaque reordering to alpha blended content.

**Validation:** Overlapping alpha, atlas growth, clipping and scissors, scrolling,
HUD scale, texture changes, particles, and exact pixels per class. Benchmark each
class separately so one product's benefit is not generalized to the factory.

## Imported geometry

### 43 Optimize full resolution imported vertex and index streams

**Finding:** Normal OBJ and glTF imports preserve source index order. Existing
meshopt cache and fetch optimization is applied to generated LODs instead.
[OBJ](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-assets/src/lib.rs#L799),
[glTF](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-assets/src/lib.rs#L1324),
[LOD optimization](https://github.com/kaz0r/Bozzard/blob/2b4351083dfb6dbb25221c75e8e3c1dabc72c835/crates/bozzard-assets/src/simplify.rs#L146).

**Implementation:** Optimize opaque full resolution geometry during cooking or
build an optimized GPU copy: deduplicate complete vertex tuples and apply vertex
cache and fetch reorder. Remap UVs, normals, tangents, joints, and weights together.
Preserve authored surface source keys and binding signatures, picking triangle
identities or explicit remaps, surface boundaries, transparent
triangle order, and rounding sensitive or coplanar primitive ordering where needed.

**Validation:** Exact images and attributes, skinned imports, material boundaries,
CPU picking, degenerate triangles, shader vertex work, and color and shadow GPU
time. This does not require reduced triangle counts or lower rendering quality.

## Delivery plan

Each wave should ship as small independently measurable changes with an explicit
reference switch where comparison needs one. Do not implement all 43 as one rewrite.

| Wave | Work | Dependencies and exit evidence |
| --- | --- | --- |
| 0 | Extend measurements before changing behavior | Baselines for active, frozen, orthographic and perspective cameras, sparse and dense movers, streaming, local shadows, interiors, crowds, text, and GI. Record stage CPU, per pass GPU, p95 and p99, allocations, write calls, bytes, plan edges, range fragmentation, and presented intervals. |
| 1 | Low complexity reductions: 9, 10, transform caching from 16, 17 to 20, 23, 40, 41 | Preserve current bytes and pixels. Demonstrate the targeted work disappears; establish warm and cold costs separately. Upload gap experiments can begin here; packed identity updates follow item 14 in wave 2. |
| 2 | Stable state and shadow foundation: 1, 2, 3, 14, remaining 16, 21, 22, 24 | Stable identities and failure safe revisions first. Retain groups and compact depth records next. Exact scene ownership, cache invalidation, and caster set parity are required. GI and catalog work remain separate conditional changes. |
| 3 | Planner scaling: 11, 12, 13, 15 | Bound construction before extending certificates. Spatial renewal and perspective reuse need randomized reference comparisons. Evaluate fullness and depth scoring independently. |
| 4 | Shadow GPU work: 4 to 8 | Coverage metadata precedes opaque pipelines. Spatial groups and accepted lists precede compaction. Recalibrate static cache thresholds after compact groups. Local static layers need measured profitability and texture budgets. |
| 5 | Visibility and shaders: 31 to 38 | Safe graph candidates and output dependency variants can be independent prototypes. Instance culling follows stable IDs and may initially use spatial subdivisions. Reject changes that cost more than they save in open views. |
| 6 | Compatibility and shared submission: 25 to 30 | Graph parameters and resource canonicalization can begin separately. Build the capability selected arena before ID indirection, geometry arenas, and multi draw. Evaluate bundles against the remaining encoding cost. |
| 7 | Product specific coverage and geometry: 39, 42, 43 | Crowd, sprite, text, HUD, transparency, and import fixtures. Shared skin outputs precede deformed batching. Geometry cooking can proceed independently with picking and primitive order safeguards. |

Suggested first substantial milestone: **1, 2, 3, 10, 14, and 16**. That gives
retained depth groups, smaller depth data, safe cheap planning, stable slots, and
sparse updates without requiring a new rendering architecture. Prototype **4 and
32** early alongside measurements because they can reduce actual GPU work; prioritize
their deployment according to pass timings and scene obstruction.

## Verification and completion criteria

Use the existing release factory active, frozen, and moving camera paired harnesses
and the renderer's unbatched or feature disabled references. Extend fixtures for
perspective motion, dense coincident bounds, membership churn, fragmented local
shadow ranges, alpha masks, and unsupported classes. Alternate mode order and use
multiple independent runs after compilation finishes.

Require exact captures and existing scene, checkpoint, ordering, and caster
contracts for unchanged rendering algorithms. Validate GPU layout, primitive
order, and backend dependent changes on representative Metal, Vulkan, and DX12
devices. If a different algorithm cannot preserve exact arithmetic, investigate
the difference explicitly rather than widening image tolerances without a reason.

Report renderer CPU, extraction CPU, direct stage costs, GPU pass timings,
synchronized waits, and presented frame intervals separately. Do not present
synchronized time or summed pass timestamps as FPS. Report cold compilation,
warm medians, p95 and p99, memory ceilings, and performance on scenes where the
optimization does no useful work. Accept architecture work only after it improves
its target workload without an unexplained regression in the baseline path.

Reference commands:

```sh
cargo test --release --locked --offline -p bozzard-render -- --test-threads=1
cargo test --release --locked --offline -p bozzard-render --test instancing \
  uniform_update_benchmark -- --ignored --exact --nocapture
cargo test --release --locked --offline -p bozzard-editor --test retained_render \
  profile_earth_factory_graph_instancing -- --ignored --exact --nocapture
cargo test --release --locked --offline -p bozzard-editor --test retained_render \
  profile_earth_factory_batch_planning -- --ignored --exact --nocapture
cargo clippy --locked --offline -p bozzard-render -p bozzard-render-assets \
  -p bozzard-scene -p bozzard-editor --all-targets -- -D warnings
cargo fmt --all -- --check
```

## API constraints checked for this plan

The pinned dependency is wgpu 30.0.1. Its [render pass documentation](https://docs.rs/wgpu/30.0.1/wgpu/struct.RenderPass.html)
requires multi draw commands to use the current render state and documents state
reset after executing bundles. Its [feature documentation](https://docs.rs/wgpu/30.0.1/wgpu/struct.Features.html)
describes indirect first instance and indirect count capabilities; they cannot be
assumed from adapter presence alone. The same constraints were checked against
the locally installed 30.0.1 sources. These facts constrain items 28 to 30; they
do not establish that those changes will improve this renderer.
