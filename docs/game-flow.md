# Game flow

Enable **Game Flow → Start, pause and retry menus** in the scene settings inspector, then set the game title and instructions. The same screen-anchored menus appear in editor Play and exported games. Existing scenes run immediately unless they opt in.

- **Ready:** click Start game or press Enter.
- **Playing:** click Pause or press Escape. Click Restart or press R to restart immediately.
- **Paused:** click Resume or press Enter/Escape; Restart or R resets the run.
- **Game over:** click Retry or press Enter/R.
- **Quit:** click Quit or press Q from a menu. This closes the player or returns editor Play to editing.

Editor shortcuts apply while the pointer is over the viewport. Losing window focus pauses an active game. Returning focus does not automatically resume it. Menu input and held movement are cleared on transitions.

Add **End Game** to a Blueprint execution chain to finish a run, optionally supplying a message (up to 240 UTF-8 bytes). This terminal action stops remaining graph actions that tick and freezes the simulation. It requires Game Flow to be enabled. Text and the final scene remain visible behind the retry menu.

Retry restores the original scene, Blueprint variables, physics, particles and cached runtime prefab templates. It does not reread changed files or retain spawned objects. Stop Play to return to the authored scene; development scenes without Game Flow retain their existing reload controls.

Scenes without a UI Canvas are migrated to ordinary editable canvas/widget objects when opened or run. Save the scene to keep that hierarchy, then change anchors/layout, images, translations, accessible labels and button Blueprints. Existing canvases take precedence. Keyboard, mouse, text scaling, high contrast, scrolling and native accessibility use the same widget layout. UI and audio completion chains can continue while gameplay is paused. See [middleware authoring](middleware.md). Gamepad navigation remains separate work.

Try `examples/demo/scenes/game-flow-lab.json`: start, tap Space three times, then retry. Pause and retry also work before finishing.
