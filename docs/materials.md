# Shared materials and shader variants

Material sources (`.material.json`) are typed assets in the scene catalog. A source
can inherit another source and override color, UV repeat, metallic/roughness,
texture, shader graph and keyword defaults. Parent and image paths are relative
to the file declaring them. Parent edits are detected by ordinary asset reload;
invalid reloads keep the last successfully decoded material.

Create a source with **New material** in the asset browser. In its Details panel,
**Create material variant** creates an inherited source. Double-click a material
or choose **Edit material source** to edit its draft. Properties have explicit
inherit/override switches. The same typed node editor edits an embedded shader
graph; **Copy selected graph** copies an existing object's graph into the source.
**Save source** validates affected descendants and live instances in a cancellable
background job before publishing an atomic file replacement. Edits remain disabled
until that save finishes. External edits or a changed scene reject stale results;
reload before retrying. Draft Undo/Redo, Revert and Close/discard are separate from
scene Undo. Text edits and drags each form one draft history step.

Assign the asset to an object with a Mesh Renderer, or select it in that object's
Material component. Per-object property and texture overrides, and keyword
choices, belong to scene history. Reset restores inheritance. Selecting Local
material returns to that component's local properties. Imported surface entities
can carry their own Material components. Legacy sub-surface selections continue
to use their existing per-surface overrides.

A local object shader graph overrides the shared source's graph. Its own defaults
replace the source's keyword defaults; instance keyword overrides then apply to
that graph. Unknown keywords are errors, including during headless scene loading
and export. Blueprint/script color writes and material tween tracks create local
instance overrides without modifying a shared source.

Static Switch and Static Switch Vector choose one of two typed input branches.
A graph declares up to eight boolean keywords with ASCII identifier names. The
compiler removes inactive and unreachable branches before generating WGSL, while
validating the entire graph, including inactive branches. Keywords are static
program choices; color and scalar property changes remain uniform changes.

The source cache holds at most 256 specialized entries and compares exact program
structure after a hash lookup. Name and position changes reuse the same program;
identical specialized WGSL shares pipeline identity even when masks differ.
Per-instance keyword layers resolve without allocating a combined map. GPU graph
pipelines are bounded to 256 active programs plus the existing small idle cache.

Import and export preserve the whole source hierarchy in a portable asset folder.
Material images use the same BC/ASTC/lossless cooking cache as other images; cache
keys use immutable source bytes, target and cooker version. Packaged sources point
to the cooked images and relative parent files. Image residency uses the material
asset ID and shares the ordinary GPU budget, upload and restoration path. Identical
material image bytes share CPU pixels and one GPU allocation across asset IDs;
property-only source edits retain that allocation. Each ID can be evicted or restored
independently, and shared storage is charged only once. Stock
shared materials also feed CPU GI transport and bake freshness; custom shader
materials are excluded from static CPU baking like local custom shaders.

Prefab bindings use ordinary asset remapping. Additive scene acquisition remaps
conflicting material IDs and their typed instance references; other global asset
IDs retain their existing conflict checks because scripts may refer to them by name.

Limits: 32 inherited source files, 1 MiB per material JSON file, 128 MiB total
material dependency bytes, eight keywords per graph. Material maps currently use
the existing texture override slot; original mesh PBR maps remain available.

Verification commands:

```sh
cargo test -p bozzard-editor --test shared_materials
cargo test -p bozzard-project --test cooking material_parents_and_maps
cargo test -p bozzard-editor-app material_ui::tests
cargo test -p bozzard-render-assets --test residency --test shader_graph -- --test-threads=1
```

The last command requires a native graphics adapter. The residency tests cover
shared allocations, replacement images, property edits, eviction and restoration;
shader tests check rendered pixels and pipeline reuse. These targeted checks do not
replace the final whole-branch checks.

Create a portable native fixture in a new directory:

```sh
cargo run -p bozzard-editor --example shared_materials -- /tmp/material-workshop
cargo run -p bozzard-editor-app -- --scene /tmp/material-workshop/scene.json
cargo run -p bozzard-player -- --export-project /tmp/material-workshop/game.bozzard.json --export-dir /tmp/material-workshop-game
```

The four cubes use a shared textured source, an inherited MATTE keyword variant
and one independent blue tint. Open the source from the Content Browser Details,
change its Color override and save: inheriting instances update, while the blue
instance retains its tint. Select that instance and disable its Color override;
scene Undo/Redo restores/reapplies inheritance. Draft Undo/Redo is available inside
the source editor. Play and Stop retain the authored bindings.

Verified on Metal on 2026-09-19: native source text editing/save, draft and instance
history, parent updates, independent overrides and Play/Stop. Exported and cooked
content-pack players each completed 20 frames from an empty working directory and
PATH with the original authoring directory unavailable.
