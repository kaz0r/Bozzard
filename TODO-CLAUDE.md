# Claude: editor and authoring UI

Planning snapshot: 2026-09-24, based on `main` at `27381da`. These are proposed tasks;
unchecked boxes do not mean work has started. Runtime counterparts are in
[Codex's list](TODO-CODEX.md). Start with C1, then C2; C3 can follow once B1's API is ready.

The next milestone is making existing engine features easier to use while building a
real game. Bozz-torio's authored menu, factory and HUD provide a useful 2D fixture;
the starter 3D scene and middleware lab cover the other workflows.

Existing foundations to preserve: hierarchy multi-selection and group movement,
edit-only visibility, asset search/filters/previews, docking, Blueprint search and
copy/paste, typed component fields, Undo/Redo, and separate Edit/Play worlds.
These already exist; this list extends them.

## C1: Canvas layout tools — P1 / M

- [x] Extend existing HUD picking/selection with persistent widget outlines,
  move/resize handles and visible anchor/pivot guides in the editor's UI canvas.
- [x] Add viewport-size presets and custom dimensions so authors can preview the
  same menu at 16:9, 16:10 and a narrow window without changing scene data.
- [x] Make parent layout constraints clear: explain when layout owns a position
  instead of letting a drag appear to work and then snap back.

**Why now:** widgets already render in the viewport, but layout authoring mainly
uses numeric fields and a few anchor presets. Bozz-torio makes this friction visible.

**Done when:** its menu and HUD can be repositioned and resized visually; each drag
is one Undo step; Escape cancels; save/reopen preserves the result; preview dimensions
do not become authored data; clicking a widget in Edit does not activate gameplay.
Check nested widgets, scrolling, clipping and UI scaling, using the runtime's resolved
layout rather than a separate layout implementation.

**Start in:** [widget inspector](apps/editor/src/widget_ui.rs),
[viewport](apps/editor/src/viewport.rs), [UI layout](crates/bozzard-scene/src/middleware/ui/layout.rs).

## C2: A true multi-object inspector — P1 / M

- [x] Show selected-object count, common components and mixed values.
- [x] Support changing shared scalar/vector fields across a selection, including
  clear absolute-versus-relative transform behavior.
- [x] Add explicit bulk add/remove component actions, with removal consequences
  shown before applying the command.

**Why now:** the hierarchy supports multiple objects, but the inspector still starts
from `selected_object()` and edits one object. Bulk hierarchy operations alone do
not let someone tune ten lights or twenty factory props together.

**Done when:** ten lights can receive one intensity edit in one Undo step; mixed
values remain unchanged until edited; invalid changes reject the whole operation;
missing components and parent/child selections behave predictably. Preserve prefab
overrides, the active document boundary and Play-mode editing guards. Reuse the
component registry rather than maintaining another field catalog.

**Start in:** [inspector](apps/editor/src/inspector.rs),
[field renderer](apps/editor/src/component_ui.rs), [editor commands](crates/bozzard-editor/src/lib.rs).

## C3: Script source pane and diagnostics — P1 / M; depends on B1

- [x] Open an attached script in a dockable, asset-backed source pane with line
  numbers, save/dirty state and explicit handling of external file changes.
- [x] Add completion/help from B1's engine API descriptions, including signatures
  and hook parameters. Do not duplicate a handwritten function list in the UI.
- [x] Show reload state, clickable file/line diagnostics and per-attachment hook
  and command counts. Make save and apply-to-running-Play distinct states.

**Why now:** Script Manager currently attaches and orders scripts; it is not a
source editor. Script changes still require restarting Play.

**Done when:** a valid edit changes `script-lab` during Play; a broken edit stays
visible with a useful error while the last valid script keeps running; closing or
switching a dirty file is handled explicitly. Stop/restart and ordinary scene saving
keep working. Active network sessions follow B1's reload restriction.

**Start in:** [Script Manager](apps/editor/src/scripts.rs),
[docking](apps/editor/src/docking.rs), [script contract](docs/scripting.md).
The pane can be developed against B1's agreed data types before reload is implemented.

## C4: Inspect and resolve prefab component overrides — P2 / M

- [x] Display inherited versus overridden values, grouped by object and component.
- [x] Add selected-component revert and a reviewable selection of changes
  to apply to the source. Keep whole-instance actions available.
- [x] Clearly distinguish an undoable scene edit from a write to a prefab source
  file, including the affected instances.

**Why now:** nesting, variants, source editing, Apply and Refresh already exist;
the missing depth is seeing and resolving specific differences before a broad write.

**Done when:** a nested variant's changed material can be reverted while preserving
its unrelated transform; selected source changes refresh correctly; local root
placement survives; invalid candidates never overwrite a source. Test unknown
components and registry-defined fields as well as built-ins.

**Start in:** [prefab inspector](apps/editor/src/inspector.rs),
[prefab commands](crates/bozzard-editor/src/prefabs.rs), [prefab contract](docs/prefabs.md).
Keep diff/command logic in editor-core so it can be tested without egui.
Start with the existing component-level override contract. Individual-field revert
needs agreed baseline/merge semantics with Codex before it becomes a UI feature.

## C5: Animator state-machine graph — P2 / M

- [ ] Draw states and directed transitions; create/select connections visually
  while retaining the existing parameter and blend-tree forms.
- [ ] Show initial state, wildcard transitions and transition priority explicitly.
- [ ] Highlight the active runtime state during Play without allowing accidental
  edits to the authoring document.

**Why now:** the runtime has state machines and blend trees, but authors edit them
through nested forms. This is an authoring view over the existing data model.

**Done when:** an idle/walk/run controller can be assembled and round-tripped through
the graph; state rename/delete cannot leave invalid references; Undo/Redo works;
rearranging graph nodes does not silently change transition priority. Store graph
layout as editor metadata, not as runtime transition order.

**Start in:** [animation inspector](apps/editor/src/animation_ui.rs),
[animation components](crates/bozzard-scene/src/middleware/animation/mod.rs).
Coordinate a read-only runtime snapshot for active-state highlighting if the current
editor adapter does not expose it; authoring the graph uses existing component data.

## C6: Editable curves, then a timeline pane — P2 / L, split into two PRs

- [ ] First make existing curve plots interactive: drag keys, edit cubic tangents,
  snap time/value, and retain the precise numeric table.
- [ ] Then add a timeline ruler, tracks, markers and camera cuts with zoom and
  an isolated edit-time scrub preview.

**Why now:** curves are plotted but edited in tables, and timeline tracks/markers
are inspector lists. The runtime already evaluates the underlying data.

**Done when:** dragging creates one history transaction and preserves valid key
ordering; cancel restores the original; scrubbing samples the same curves as Play
without firing gameplay events, saving preview poses or mutating the authored scene.
Agree the preview API with Codex before implementing a second simulation path.

**Start in:** [motion inspector](apps/editor/src/motion_ui.rs),
[middleware contracts](docs/middleware.md).

## Delivery rules

Use one focused PR per task, splitting C6 as described. Claude owns editor widgets,
interaction, accessibility and small editor command adapters; Codex owns shared
runtime contracts. Agree overlapping editor-core changes before both agents edit
the same module.

For each feature, preserve keyboard navigation, popup focus, DPI behavior, persisted
docking and Edit/Play isolation. Exercise meaningful command regressions plus the
actual native UI, including Undo/Redo, save/reopen and a narrow window. Measure large
selections/graphs before and after; cache derived state by revision instead of
rebuilding it every repaint. Use the normal checks for changed code and wait for CI
before calling a PR ready.
