# Editor layout and custom inspectors

Each workspace panel has a Dock menu. Move it into the left top, left bottom,
right, bottom or center group, or detach it into a floating window. Panels in
one group become tabs. Drag a tab onto another group's tab bar to dock it there;
empty destinations appear while dragging. Resize the group borders as needed.
Closing a floating window returns its panel to its default group.

View controls panel visibility and Reset docking layout restores the default
arrangement and sizes. Panel locations, active tabs, visibility and floating-window
geometry persist between editor launches. Layout preferences belong to the editor
workspace; they do not alter scene files or game exports.

The editor application is also a Rust library. A game-specific executable can
register its component types, install custom inspectors and then call
`bozzard_editor_app::run_with_inspectors`. Its ordinary `--scene`, `--project` and
backend arguments remain available. The stock executable uses `run()`.

`custom_inspectors::Registry::register` associates a component's scene key with an
inspector function. Register the `bozzard_scene::ComponentType` first. Existing
components with generic field metadata can also have a custom inspector. Unknown
component names and duplicate inspector registrations return errors.

An inspector receives an egui UI, the component's JSON value and read-only object,
scene and asset context. Draw the desired widgets and update the value when an
input changes. The editor loads edits through the registered component schema,
validates the resulting scene and applies its ordinary gesture-coalesced Undo/Redo.
Returning an error or an invalid component value discards the edit. Disabled/Play
inspectors cannot publish edits. The component's existing scene, prefab and export
serialization remains responsible for persistence; UI callbacks stay in the editor.

`Registry::new()` starts empty. `Registry::default()` includes the stock custom
Spin inspector, which shows angular rates and rotation presets. Both fall back to
generic field controls for unregistered generic components.

The buildable example replaces Spin's controls with revolutions-per-minute sliders:

```sh
cargo run -p bozzard-editor-app --example custom_editor -- --scene examples/demo/scenes/asset-lab.json
```

Use an existing scene path and select an object with Spin, or add Spin from the
Inspector's Add Component menu. The example lives in
`apps/editor/examples/custom_editor.rs`. It demonstrates the same extension point
available to a registered game-owned component without adding egui dependencies
to headless scene or runtime crates.
