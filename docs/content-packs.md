# Content packs

A content pack is a self-contained set of cooked scenes and assets. A catalog maps
stable addresses such as `levels/workshop` or `props/courier` to immutable pack
generations. Games can install a pack from a local release folder or an HTTPS
server, then resolve its contents without the authoring project.

## Build a release

The checked-in [demo specification](../examples/demo/content.json) contains three
scenes, an animated model and an audio asset:

```sh
cargo run -p bozzard-project -- bundle examples/demo/content.json /tmp/bozzard-demo-content
cargo run -p bozzard-project -- list-content /tmp/bozzard-demo-content/catalog.json
cargo run -p bozzard-project -- fetch-content /tmp/bozzard-demo-content/catalog.json levels/workshop /tmp/bozzard-content-cache
cargo run -p bozzard-player -- --content-catalog /tmp/bozzard-demo-content/catalog.json --content levels/workshop --content-cache /tmp/bozzard-content-cache
```

The release folder must not already exist. It contains `catalog.json` and
`content.bpack`; keep both together when moving the release. The player selects
the scene's available view, preferring 3D when it has both, and retains its mounted
pack for the lifetime of the game. Content arguments cannot be combined with
`--scene`, `--project` or export arguments. Without `--content-cache`, installation
uses the user's platform cache directory.

The editor's **File → Build content pack…** chooses a specification and output
folder, shows background progress and supports cancellation. It builds saved
source files; save scene changes first. Existing release folders are retained.

An authoring specification uses paths relative to its JSON file:

```json
{
  "version": 1,
  "id": "chapter-one",
  "name": "Chapter One",
  "cook": "universal",
  "scenes": { "levels/start": "scenes/start.json" },
  "assets": { "props/hero": { "kind": "mesh", "path": "models/hero.glb" } }
}
```

Choose `rgba`, `bc`, `astc` or `universal`. Packs require cooking: source OBJ/glTF
models become self-contained BMESH payloads, including geometry, PBR maps, skins
and clips. Images become PNG or BTEX. Prefab and runtime-scene dependencies are
included; source IDs and internal object references are preserved. Shared source
assets are emitted once per pack. The same dependency/target/codec keys used by
[project export](texture-compression.md#models-and-project-export) reuse unchanged
model/image outputs from `.bozzard-cache/cook-v1` beside the specification.

Addresses are case-sensitive, at most 128 ASCII characters, with nonempty `/`
segments of letters, digits, `_` and `-`. Pack IDs are a single segment. A pack
holds at most 1,024 addressed entries, including at most 64 scenes. Asset entries
use the same typed kinds as scene asset catalogs.

Scenes without cameras can be addressed as additive chunks. Runtime loading and local
scene export accept these entries; standalone player startup requires an entry with a
starting view. The catalog omits `view` for a camera-free chunk.

## Download and resolve

Upload the two release files together to an HTTPS server, then give the player the
catalog URL. Relative pack locations resolve against the catalog's final URL,
including redirects. The catalog can combine several releases by copying their
pack records and address mappings and adjusting the locations. Each record pins
the bundle's exact size, SHA-256 and index SHA-256.

HTTP is supported on loopback only, for local development. For example, serve a
release with `python3 -m http.server 8000 --directory /tmp/bozzard-demo-content`,
then use `http://127.0.0.1:8000/catalog.json`. URLs cannot contain credentials or
fragments. Up to five redirects are followed; HTTPS cannot downgrade to HTTP.
Fetching content runs no game ticks, but scenes can contain gameplay scripts:
choose catalogs from publishers whose game code you intend to run. Checksums
verify the catalog's content references; they are not a publisher signature.

Rust callers use `bozzard_project::content`:

```rust,no_run
use bozzard_assets::job::Progress;
use bozzard_project::content::{ContentStore, load_catalog};

# fn example() -> anyhow::Result<()> {
let progress = Progress::default();
let catalog = load_catalog("/path/to/release/catalog.json", &progress)?;
let mut store = ContentStore::new("/path/to/cache");
let content = store.resolve(&catalog, "props/hero", &progress)?;
let source = content.asset_source()?;
// The AssetSource path is relative to content.pack().root().
// Keep `content` (or its Arc<MountedPack>) while using the resolved files.
# Ok(())
# }
```

`scene_view()` rejects asset addresses and `asset_source()` rejects scene
addresses. A store verifies a generation once and shares its mounted handle for
subsequent resolutions. A new store rechecks the installed files and typed data.
An already loaded catalog can resolve an installed pack while offline. Catalog
downloads themselves are explicit and are not silently replaced by cached data.

These APIs are blocking worker operations. Use `bozzard_assets::job::Job` for
interactive hosts; its label reports the current file/stage, and cancellation is
checked between reads, assets and validation stages. Connection/header waits and
individual body reads are bounded to five seconds. A download may take up to ten
minutes overall while making progress. Codec calls finish their current bounded
operation before cancellation is observed. Player startup resolves content before
opening its window; gameplay scene transitions are a separate loading workflow.

## Installation and limits

Each immutable generation lives at `<cache>/<bundle-sha256>`. Downloads stream in
64 KiB chunks into a unique staging directory. Header/index bounds, sorted portable
paths, case collisions, parent-file collisions, exact lengths and every file hash
are checked before publication. Every transitive asset reference must remain in
the bundle inventory. Scenes, runtime scenes, loose assets and prefab dependencies
receive typed validation. Meshes must already be BMESH. Files are never exposed
from a partial installation.

An OS lock serializes installs and repairs of the same generation. A cancelled
wait releases its worker; a process exit releases its lock. Failed/cancelled
staging is removed. Cancellation immediately after a completed install may leave
that valid generation cached, while preventing its delivery to the host. Cache
verification reconstructs the whole archive checksum in the same file-reading
pass used for individual checksums. A corrupt installation is replaced only after
a new candidate passes validation.

Updating a catalog creates a new generation; existing handles and the previous
generation remain available. Cache generations are not automatically pruned.
Remove unused generation directories only after their readers have released them.
Do not edit installed files in place. To author a packed scene, copy its full
generation to a project directory first.

Limits are 1 MiB per catalog, 8 MiB per index, 8,192 files per bundle, 512 MiB per
file and 4 GiB per bundle, with the asset decoders' tighter limits still applying.
A catalog holds at most 1,024 packs and 4,096 addresses. Filenames in the bundle
are portable ASCII. Archive compression is unnecessary for already compressed
texture payloads; downloads do not allocate an entire bundle in memory. Typed
asset validation still needs memory for decoded assets.

`cargo test -p bozzard-project --test content --test cli` exercises relocated
builds, incremental reuse, address resolution, cache repair, concurrent installs,
catalog updates, malformed typed content and HTTP streaming/failure/cancellation.
The HTTP tests use a local loopback server and need permission to bind a socket.
