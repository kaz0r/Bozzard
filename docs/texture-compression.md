# Cooked textures

PNG/JPEG imports retain exact RGBA rendering. To reduce GPU memory for an authored
image, cook a `.btex` asset and import it using the editor's **Import** dialog:

```sh
cargo run -p bozzard-project -- cook-texture universal path/to/paint.png path/to/paint.btex
```

`bc` writes BC3, `astc` writes ASTC 4×4, and `universal` writes both. The optional
last argument is `srgb` (default, color textures) or `linear` (data maps). The
destination must be new. Cooking preserves the source file. Both encoders are CPU
tools; no GPU is needed to cook or validate the result.
Standalone image assets sample as sRGB; a linear-only payload is intended for a
model data map and uses the RGBA fallback when assigned as an ordinary color image.

BTEX stores full mip chains, the original pixels as a lossless PNG, a versioned
bounded header, and a SHA-256 checksum. Width and height are limited to 4,096 and
the file to 32 MiB. Malformed dimensions, formats, mip counts, lengths, checksums,
and duplicate variants fail import. Original pixels continue to serve thumbnails,
picking, GI, and a lossless fallback on devices without a supported cooked format.

The editor and player request supported BC/ASTC device features. Staged uploads
prefer an available ASTC payload, then BC3, then RGBA. Base dimensions must be
multiples of four for block-compressed GPU textures; other dimensions keep RGBA
without resizing or changing UVs. Compressed payloads contain alpha. sRGB mip
filtering operates in linear space with premultiplied alpha; linear data maps
filter independent channels. Compression is lossy, so inspect fine details and
alpha-tested edges before replacing a source texture.

Asset details show available formats, mip counts, and chain sizes. A standalone
image currently uploads only its base level, matching ordinary image sampling;
embedded model maps upload their full chains. The GPU assets budget counts block
bytes, including padded mip tails. Eviction/restoration uploads saved blocks and
does not rerun an encoder. Integrity checks run on the resource preparation worker.

Scene assignment, history, save/load, last-good reload recovery, and relocation
work as for PNG/JPEG. Shared model maps are deduplicated by image identity and
color space; the renderer retains an exact RGBA reference upload path.

## Models and project export

```sh
cargo run -p bozzard-project -- cook-model universal path/to/model.glb path/to/model.bmesh
```

The source may be OBJ, glTF, GLB or an existing BMESH. Targets are `rgba` (lossless
maps), `bc`, `astc` and `universal`. BMESH preserves vertex/index streams, source
surface identities, material factors and samplers, shared maps, skin bindings,
rigs and animation clips. Color and data uses of the same image receive separate
mip chains. The native payload is bounded and checksummed, with a 512 MiB file
limit and existing geometry/image/rig limits. It imports through the normal
editor dialog. The runtime reads the cooked streams without parsing OBJ/glTF or
running texture encoders.

**File → Export game → Asset cooking** cooks all declared meshes and images,
including dependencies of spawn prefabs and the runtime scene library. The
portable choice writes BC3 and ASTC; lossless fallback pixels always travel with
them. Existing authored files and asset IDs are preserved. The export is validated
in a staging directory before publication. Cancellation discards the staged game.

For CLI export, set `"cook": "universal"` in `bozzard.project.json`, then use the
normal `bozzard-player --export-project ... --export-dir ...` command. The other
values are `source`, `rgba`, `bc` and `astc`. Older manifests without this setting
keep original assets; new project templates and the editor export dialog default
to `universal`. This selects the asset representation; the native executable still
targets the machine on which it was built.

Cooked outputs are cached in `.bozzard-cache/cook-v1` beside the starting scene.
A SHA-256 key includes source bytes, external resource names and bytes, target and
cooker/codec version. Moving the project does not invalidate keys. Cache hits skip
source decoding and encoding. Writes use temporary files and verified payloads;
corrupt entries rebuild. The exporter reports cooked/reused/copied counts. Cache
files are disposable, excluded from Git, and never included in the exported game.
Delete that cache folder to reclaim disk space or force a complete rebuild.

Cooking preserves the exact CPU geometry/materials/pixels used for baked lighting.
A current GI bake is checked against the cooking inputs and rebound to the exported
asset fingerprints; stale bakes stay stale. Original scenes are never rewritten.
Addressable downloadable packs remain part of the unfinished content pipeline.

## Verification

```sh
cargo test -p bozzard-assets texture::tests
cargo test -p bozzard-render-assets --test residency
cargo test -p bozzard-editor --test textures
cargo test -p bozzard-project --test cli
cargo test -p bozzard-project --test export --test cooking
cargo test -p bozzard-assets cooked_model::tests
cargo test -p bozzard-render-assets --test cooked_models
```

On Apple M2 Pro/Metal, a 32×20 gradient fixture uses 640 base-level GPU bytes
instead of 2,560. Against the RGBA render, channel RMSE is 3.090 for BC3 and 0.485
for ASTC on a 0–255 scale. This fixture measures memory and visual error, not FPS.
Native tests also exercise model mip tails, shared sRGB/linear maps, exact byte
accounting, RGBA fallback, and pixel-identical restoration of compressed resources.

The cooked courier model on the same Metal adapter uses 1,392 instead of 5,460
texture bytes (all mip levels), with channel RMSE 0.542. Its lossless fallback is
pixel-identical. A skinned banner preserves sampled poses and native animated
frames through cooking. These are fixture measurements, not general FPS claims.

Encoder sources: [texpresso 2.0.2](https://docs.rs/texpresso/2.0.2/texpresso/)
and [ctt-astcenc 0.5.0](https://docs.rs/ctt-astcenc/0.5.0/ctt_astcenc/), the latter
vendoring Arm's ASTC encoder. No encoder runs on render frames.
