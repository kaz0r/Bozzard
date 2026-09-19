# Export a native game

Bozzard exports a folder containing a native player, a starting scene and its assets.
The recipient opens **Game.app** on macOS, **Game.exe** on Windows, or **Game** on Linux.
No Rust, Python, source checkout or command-line scene argument is needed to play.
Keep the folder's contents together; the folder can be moved or renamed.

## From the editor

Build the editor and its companion player in the same profile:

```sh
cargo build --release --locked -p bozzard-editor-app -p bozzard-player
target/release/bozzard-editor --project examples/demo/first-trail.bozzard.json
```

Author a scene, use **Play/Stop** to try it, then choose **File → Export game…**.
Enter a game name and use **Choose folder…** to pick a location in the native folder picker.
The dialog shows the target OS/architecture and previews the game folder it will create.
For example, exporting **First Trail** twice creates **First Trail** and **First Trail (2)**
inside the chosen location. Existing exports remain intact; no path typing is needed.
The chosen location is reused for subsequent exports in the same editor session.

Export includes current authored edits, including unsaved edits. During Play it exports the
authored scene, preserving the running editor world. Export runs in the background and
supports cancellation before publication. Once complete, **Game exported** offers
**Play game**, **Open folder**, and **Done**. Launch errors appear in that window.

The full native development bundle also includes the companion runtime. An editor copied
by itself shows the missing player in the export dialog and disables export until it is installed.
The player must be from the same engine build as the editor.

## Steam games

The standard editor/player include Steam support. Cargo stages the native API library;
no Python launcher or manual SDK setup is needed to use editor Play. **File → Export game…**
automatically includes the matching library beside the game executable, including in the
macOS app bundle. It is covered by `package.json` and described in `steam-runtime.json`.
Steam-enabled runtimes need this library even when the exported scene is single-player.
The library can remain dormant: solo scenes do not initialize Steam or require its client.
Use `--no-default-features` when building the editor/player for binaries without Steam SDK support.

The Steam Multiplayer component exposes **Steam App ID**. Spacewar 480 exports include
development settings and open directly with Steam running. Other IDs produce Steam store
builds without `steam_appid.txt` or App ID environment overrides. Configure the exported
executable in your Steam launch options; use editor Play for local tests. Multiplayer export
checks the companion player's Steam support, SDK and platform before publishing. See
[Steam testing and distribution](multiplayer.md#scenes-scripts-and-export).

## Project manifest and command-line export

The minimal JSON manifest selects the starting scene and camera view:

```json
{
  "version": 1,
  "name": "First Trail",
  "start_scene": "scenes/first-trail.json",
  "view": "3d"
}
```

`start_scene` is relative to the manifest and must remain inside its directory. The scene's
asset catalog declares the content to package. All declared assets are included, together
with transitive prefab dependencies and external glTF/GLB/OBJ buffers, material libraries
and supported images. Compressed audio and scripts are included too. Blueprint attachments,
cooked animation rigs, navigation bakes, curves and UI layouts are serialized in scenes and prefabs.
Original-source exports preserve model bytes and dependency names. Cooked exports preserve
surface identities and rebind current baked GI fingerprints to their lossless CPU data;
BC3/ASTC payloads reduce GPU texture storage. See asset cooking below. The standard compiled runtime includes the current
scene, physics, Blueprint, scripting, middleware and rendering systems.

Export with the native player directly (no Python required):

```sh
target/release/bozzard-player --export-project examples/demo/first-trail.bozzard.json --export-dir dist/first-trail
```

Or create a folder and deterministic ZIP, then verify extraction and gameplay:

```sh
python3 tools/package.py --project examples/demo/first-trail.bozzard.json \
  --export-dir dist/first-trail-macos-arm64 --verify --verify-first-trail \
  --window --hardware --backend metal
```

Use a fresh output name on repeated exports. Other platforms use `vulkan` or `dx12`;
omit `--hardware` or request `--software` on supported software adapters.
`--verify-first-trail` is specific to the reference First Trail route. Omit it for other games.
The existing `tools/package.py` mode without `--project` still creates the engine demo bundle.

## What verification proves

Export validates the starting camera, scene, prefab runtime and decoded assets before making
the output folder visible. Failed or cancelled work removes its staging directory. `package.json`
contains the host OS/architecture, engine version, executable path and sorted file byte counts.
It is an inventory, not a cryptographic integrity or signing manifest. ZIP entries have stable
timestamps and ordering; identical inputs and folder names produce identical archives.

`--verify` extracts and renames the ZIP outside the repository, restores executable permissions,
checks its inventory and launches the extracted binary from an empty working directory with
an empty tool search path. It passes no scene/project path. The GPU smoke checks exercise the
packaged scene and assets. `--window` also opens the native game window.

First Trail verification feeds physical W and a Space press through the player's input adapter
for 340 fixed ticks. It requires three collectibles, the checkpoint, victory, zero falls, and
a successful physical-R restart. With `--window`, these ticks are rendered in the actual player.
This is deterministic input-adapter coverage, not automated OS keyboard/focus/mouse gestures.
The native editor smoke also checks background export of unsaved authoring changes during Play
and the completion dialog. Folder-naming regressions cover repeated exports and portable names.
CPU regressions remove their temporary source trees before loading exported assets and completing
the level, and check model/prefab relocation, unchanged asset fingerprints and failure cleanup.

CI exports First Trail on Linux, Windows and macOS; Linux additionally presents the route under
Xvfb. The manual hardware workflow presents the exported route on its selected native desktop.
Workflow definitions do not establish that a given revision has passed remotely.

## Host and distribution scope

An export runs on the OS and architecture for which its player was built. The local macOS
Apple Silicon build requires Metal and compatible system frameworks. Windows requires the
native runtime's system libraries and DX12 drivers; Linux requires the Vulkan/window libraries
listed in CI. Cross-target release artifacts are produced by their native CI jobs, not by this
host exporter. Minimum supported OS versions are not yet certified.

These local folders are suitable for playing and testing. Public releases still need dependency
and asset license notices, certified OS baselines, platform packaging, and distribution signing
(including Apple Developer ID signing and notarization for macOS). The exporter does not obtain
certificates or claim notarization. Projects should supply their own player instructions in the
exported README before distribution; the generated control hints cover Bozzard's default controller.

## Asset cooking

The editor export dialog offers portable BC3 + ASTC cooking, either individual
format, lossless model cooking, or original sources. Project manifests use the
optional `cook` field (`universal`, `bc`, `astc`, `rgba`, `source`). Old manifests
default to original sources; new project templates default to portable cooking.
See [texture and model cooking](texture-compression.md#models-and-project-export)
for format selection, incremental cache behavior and verification commands.
