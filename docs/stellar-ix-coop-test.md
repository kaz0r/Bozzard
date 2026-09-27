# Stellar-IX: two-computer Steam test

Use the **same release package** on both computers. This checks real guest connections;
the automated tests have already exercised the protocol, factory rules, and a real
single-member Steam host, but those checks do not establish that live guests work.

## Setup

1. Use two Linux x86-64 computers with Steam running and signed in to **different
   accounts**. The accounts should be Steam friends and online. This development
   build uses App ID **480 (Spacewar)**, so Steam may show that name.
2. Extract the complete package on each computer. Keep `Game`, `steam_appid.txt`,
   `libsteam_api.so`, the project, and assets together. Compare `BUILD.txt` on both
   computers; the build ID must match. `sha256sum -c SHA256SUMS` verifies the package.
3. Add **`Play-test.sh`** from the extracted folder to Steam as a non-Steam game,
   enable Steam's in-game overlay (including for that library entry), and launch
   that entry on both computers. The launcher uses
   release code, stores test saves in `test-data/stellar-ix/saves`, and records a new
   log in `test-data/logs`. Your ordinary editor/player saves remain separate.
4. Choose which account will host. Keep these roles for the first pass. Use a new
   **Creative** world so recipes, inventory, and the completed rocket are available.
   Creative machines still require a working power connection.

The supplied binary is Linux-only. A Windows or macOS computer needs a native export
of this same source version; it cannot run this Linux executable directly.

## Steam overlay

Open **Steam co-op** and check the overlay status at the bottom. Once ready,
**Open Steam overlay** opens Steam's Friends view. **Shift+Tab** is Steam's default
shortcut; use your configured Steam shortcut if you changed it. **Invite friends**
opens Steam's lobby invite dialog after creating a lobby. The friend picker remains
available when the overlay is disabled or unavailable.

While a powered route is running, open the overlay while moving or gathering. Leave
it open for a few seconds: factories should keep producing, but overlay clicks and
typing must not move, build, gather, or open game interfaces. Close it and check that
movement works again without a stuck key or drag. Repeat in editor Play if testing
the editor. An editor opened with a blank scene may need a restart with the factory
project selected so Steam initializes before graphics creation.

If the status stays unavailable, exit the game, check Steam's overlay setting and
launch the library entry again. Running `./Play-test.sh` directly still supports
solo play and co-op, but does not guarantee Steam injects its overlay. App ID 480
is shared development infrastructure: do not launch Spacewar itself to start this
game. See [Valve's overlay requirements](https://partner.steamgames.com/doc/features/overlay).

## First pass: join before starting

For the guest movement regression, tap several directions quickly before waiting
for the host. The guest's own cursor should respond on the next local simulation
tick, stay in place while idle, and avoid jumping backward when replies arrive.
Alternate mouse tile selection and WASD, cross an already revealed chunk boundary,
then move and place a belt: both computers should agree on the final position and
build tile. Newly explored terrain still comes from the host.

| Step | Action | Expected result |
| --- | --- | --- |
| 1 | Host: **Steam co-op → Create lobby** from the title. | A friends-only lobby appears with the host and space for three guests. |
| 2 | Host: **Invite friends**; use **Friend picker** if the Steam overlay does not open. Guest: accept the Steam invitation while the game is running. | Both screens show the two members. Guest waits for the host to choose a world. |
| 3 | Both: send a short message using the lobby **Chat** control or **Enter**. | Both see each message once, with the correct name. |
| 4 | Host: close the co-op panel, choose **Creative**, then **Create world**. | Both enter the same world. Host is blue; the first guest is red. |
| 5 | Move independently with **WASD**, then stand close together. | Each player controls only their own marker. Nearby names appear; names hide when far apart or on different planets. |
| 6 | Press **Enter**, type `wasd jime 123`, and send with Enter. Repeat, but cancel typing with **Escape**. | Typing neither moves the player nor opens interfaces or changes tools. Movement works again afterward without any stuck key. |
| 7 | Guest explores a new region while host stays behind. Both open **M** on the same planet. | Host learns the guest's discovery. Explored regions agree; loaded-region counts may differ because the cameras are independent. |

Also try a long message: the chat history and draft should scroll, keeping new text
visible. Scrolling back should remain possible while no new message is arriving.
Long member names and the friend picker have their own scroll areas.

## Factories and independent planet travel

Build a small, powered **miner → belts → storage** route. On the Power bar
(`Ctrl+3`), use slot **2** for a pole and slot **3** for cables. Connect the landing
pod to the pole, then the pole to the miner. Stand at each terminal and press
**Space** with the cable tool. Every cable needs a pole at one end; poles have five
connections. A miner uses one power and the pod supplies eight.

1. Let both players observe the route. Storage contents should increase, and belt
   items should move smoothly on both screens. Guest observation must not create
   extra items.
2. Extend a route across a region boundary, including the wiring if necessary.
   Move both players away from the miner's region. It should keep producing and
   transferring across the boundary. A full destination should stop the route
   without losing items.
3. Have the guest build, rotate, and remove a spare machine. Both screens should
   agree. Then both try to build on the same empty tile: only one machine should
   exist there. Do not use the working route for this conflict check.
4. Have the guest open an assembler or Constructor with **E**, choose a recipe,
   load ingredients, and collect output. Check storage and **I** as well: inventories
   belong to each player; a collection must transfer items only once.
5. Record the Earth storage quantity. Host boards the rocket with **E** and chooses
   **Launch to Moon**; guest stays on Earth and watches the route. Earth production
   and belt animation should continue while the host is on Stella-Z2.
6. Guest then visits the Moon. Leave Earth running for about a minute with room in
   storage, return, and compare the quantity. It must have increased while nobody
   was on Earth. Repeat with a powered lunar miner/storage route while both players
   are on Earth. Machines must still obey power and capacity limits.
7. Leave an Escape menu, journal, or inventory open for a while. Production should
   continue. Check that the mouse and keyboard operate only the open interface.

## Save, load, disconnect, and late join

1. Host opens **Escape → Save**, chooses a manual slot, and saves. Confirm the slot
   shows the world's progression and cycle information. Guest cannot save or load
   the shared world.
2. Record each player's carried items and location, a storage quantity, and a spare
   machine's position. Make a few changes, then have the host **Load** that slot while
   the guest stays connected. Both should return to the saved world and their own
   saved inventories. Old requests must not reappear after the load.
3. Give the guest a distinctive stack to carry. Guest opens
   **Escape → Steam co-op / Invite friends → Leave lobby**. Keep the host running,
   invite that guest again, and accept. The guest should receive the current world
   and their own retained inventory, with no duplicate player marker or items.
4. Test a cold late join: guest exits their game, host keeps producing, guest starts
   `Play-test.sh` again, and host sends another invitation. Joining should not reset
   the host's world or halt production.
5. For a separate host-starts-alone check, both leave the lobby and return to the
   title. Host creates a new lobby and loads the manual slot **before** inviting the
   guest. Guest should join that already-running saved world.
6. Let one session run for **at least 20 minutes after the last manual save/load**.
   Check that Autosave has appeared/updated with plausible cycle and progression
   information. Only the host should write it. Loading Autosave should restore the
   world on both computers.
7. Host leaves the lobby while the guest is connected. Guest should be told the host
   has left rather than silently continuing a separate copy of the factory. Verify
   both can return to the title and establish a fresh session.

For the progression check, start a separate **Survival** world. Have the guest gather
and deliver the first journal requirements: **12 iron ore + 8 copper ore + 8 stone**.
Both should gain the first automation unlocks, and a host save/load should retain
them. Creative alone cannot verify shared progression.

## Optional four-account pass

With two more computers/accounts, repeat join and movement. Player 3 should be
orange and Player 4 green; all four should chat, build, and explore independently.
A fifth account must not join. Disconnect and rejoin one guest while the other three
keep playing. The two-computer pass does **not** verify these cases.

## Report back

Send `BUILD.txt`, each computer's log from `test-data/logs`, and this short record:

```text
Build ID:
Host OS / GPU:
Guest OS / GPU:
Join before start: pass / fail
Chat and keyboard capture: pass / fail
Shared discoveries and progression: pass / fail
Guest building / recipe / collection: pass / fail
Cross-region factory: pass / fail
Different planets / nobody on Earth: pass / fail
Manual save / connected load: pass / fail
Rejoin / cold late join: pass / fail
Start alone then invite: pass / fail
20-minute autosave: pass / fail / not run
Host leaves / new session: pass / fail
Four-player pass: pass / fail / not run
First failing step and what each screen showed:
```

For a freeze, mismatch, or visual problem, capture both screens with the debug HUD
visible and note which player was on which planet. Include FPS, CPU draw, Sim worker
CPU, and Wait. A successful test means the stated behavior was observed on **both**
computers; an invitation arriving on its own is not enough.
