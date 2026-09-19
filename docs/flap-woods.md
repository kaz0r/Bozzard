# Flap Woods

Flap Woods is the first Blueprint game using the HUD and shared game-flow menus.

Open `examples/demo/scenes/flap-woods.json` in the editor and press Play, or launch:

```sh
cargo run --release -p bozzard-player -- --project examples/demo/flap-woods.bozzard.json
```

Click **Start game** or press Enter. Tap Space to flap through the thorn pipes. Each completely cleared pair earns one point. Escape pauses/resumes; a collision with a pipe, floor or ceiling ends the run and retains the final score. Click Retry or press Enter/R to reset the bird, pipes, score and Blueprint state. Quit exits the player or stops editor Play.

The score and control hint remain screen-anchored when the window changes size. Pipe gaps change as the three pairs recycle. The score sensor behind the bird recognizes each bottom pipe by its object reference, so the two halves award one point together; the floor, ceiling and bird proxy cannot award points. Death uses the terminal End Game node instead of automatic respawn.

Open the scene in the editor and choose **File → Export game…**, or run:

```sh
cargo build --release -p bozzard-player
python3 tools/package.py --project examples/demo/flap-woods.bozzard.json \
  --export-dir dist/flap-woods --verify --verify-flap-woods
```

Use a new export directory if the folder or ZIP already exists. Add `--window` on an active desktop to check native presentation too. The verification extracts the ZIP, renames its folder, removes build tools from PATH, and launches from an unrelated directory without a scene/project argument. The physical-key route checks Start, a cleared pipe and score, pause/resume, death, retry, restart and Quit. GPU smoke checks exercise the packaged scene and renderer separately.

The CPU gameplay regression also plays 1,800 ticks through multiple pipe recycling cycles. CI runs the exported route on macOS/Metal, Linux/Vulkan and Windows/DX12; Linux additionally checks the native window under Xvfb.

Regenerate the procedural scene, then normalize it with the player serializer:

```sh
python3 tools/gen_flapwoods.py
target/release/bozzard-player --scene examples/demo/scenes/flap-woods.json \
  --write-scene examples/demo/scenes/flap-woods.json
```

Audio, custom menu styling and saved high scores are not included in this example.

## Play with Steam friends

The separate [Flap Woods Together](multiplayer.md) variant supports 2–4 players, Steam
lobbies and friend invitations, and host-only start/retry using Spacewar App ID 480.
Run `python3 tools/steam.py run`; the launcher builds and stages the required Steam runtime.
The original scene above retains its solo gameplay.
