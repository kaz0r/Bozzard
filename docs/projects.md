# Projects and scene merges

Use **File → New project…**, enter a name and a new folder inside an existing
directory, and choose **3D exploration** or **2D collect game**. Creation refuses
an existing destination. The editor opens the starting scene through its normal
unsaved-change handling. Each project contains its manifest, scene, assets,
controls, and export instructions. Press Play, then Start.

The same templates are available without a graphical editor:

```sh
cargo run -p bozzard-project -- new third-person /tmp/my-3d-game "My 3D Game"
cargo run -p bozzard-project -- new collect-2d /tmp/my-2d-game "My 2D Game"
cargo run -p bozzard-player -- --project /tmp/my-2d-game/bozzard.project.json
```

The 3D template includes a capsule controller, platforms, collectibles and a goal.
The 2D template includes movement, a score HUD and a Rhai controller in
`scenes/assets/controller.rs`; the score lives in the shared scene blackboard.
These are independent project directories and need no demo-folder assets.
Ready-made copies live in `examples/starter-3d` and `examples/starter-2d`, each
with its own `bozzard.project.json` and starting scene.
Use the usual [export workflow](exporting.md) to package either project.

## Three-way scene merge

```sh
cargo run -p bozzard-project -- merge base.json ours.json theirs.json merged.json
```

Inputs must each be valid Bozzard scenes. The tool merges fields recursively,
matching scene objects and other ID-bearing array elements by persistent ID.
It combines independent component edits, additions and deletions, preserves a
unilateral object reorder, and retains unknown component data. Non-ID arrays
(such as a vector or attachment list) are atomic values.

Conflicting edits, delete-versus-edit, different new objects sharing an ID, and
competing object reorders require a decision. The command exits with an error
and writes `merged.json.conflicts.json`, containing paths and base/ours/theirs
values plus a reviewable candidate. An absent side property means deletion;
an explicit `null` remains a JSON null value. A conflict candidate is never
written as the requested scene.
Resolve the conflicting input edits and rerun with a fresh output filename.

The combined result is validated again: for example, two individually valid
reparentings can create a cycle together. Such a merge also requires correction.
Input files and existing output/report files are never overwritten. The helper
does not move assets; all input documents must use the same project-relative
asset paths. Review the result in the editor before replacing a working scene.
