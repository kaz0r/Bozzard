# Prefabs

Prefabs are authoring assets that keep a reusable object hierarchy in a separate JSON file while scenes retain expanded objects. Headless runs use those expanded scene objects without reading the authoring prefab source. The graphical player loads the scene's declared asset catalog, so the prefab files must still be present, but it does not perform editor prefab expansion.

The current workflow is:

1. Select the root object of the hierarchy to reuse, then choose **Save as prefab** in the Inspector. The editor creates `assets/<name>-prefab-N.prefab.json`, chooses the first unused `N`, and links every object in the selected subtree to that source.
2. In the Assets panel, filter by **Prefabs**. Choose **Add to scene**, or drag the prefab card into the viewport. In 3D, drag placement uses the y=0 construction plane; in 2D it uses z=0. A near-parallel or behind-camera ray falls back to a point five units ahead of the camera. Each placement gets fresh scene object IDs and remains linked to the same source. Placement is local to the instance.
3. Edit an instance in the Inspector. Component-level changes are retained when the source is refreshed through a three-way merge against the instance's saved baseline.
4. Select a linked instance and choose **Apply to prefab** to write its hierarchy back to the source. The selected instance's root placement is kept in the scene, while the source receives the reusable hierarchy. Other linked instances of that source in the current scene update as part of the same operation.
5. Choose **Refresh instances** after changing a prefab file outside the current operation, or after applying the source from another scene. Reopening a different scene does not refresh its instances automatically: explicitly refresh after the other scene has applied the source. Refresh merges source changes into all linked instances in the current scene.
6. Choose **Unpack** to remove the link and keep the expanded objects. Unpacking is undoable; subsequent edits to the expanded hierarchy are ordinary scene edits.

The checked-in prefab lab demonstrates three linked Cargo crate roots (five objects each):

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/prefab-lab.json
```

Select the first **Body**, change its tint, and choose **Apply to prefab**. The second body follows the source change, the orange third body keeps its local `Drawable` override, and every root placement remains unchanged. This test writes the example's `examples/demo/scenes/assets/cargo.prefab.json` source asset; use your own Save-as-prefab copy for experiments or restore the example with Git afterward.

Applying a prefab overwrites the source file after preparation succeeds. The scene change is recorded in editor history, but the source-file write is not undone by scene **Undo**. Treat **Apply to prefab** as an explicit source edit and keep normal file backups or version control for source recovery.

The merge keeps a local component when it differs from the saved baseline; an unchanged local component receives the newer source component. Transform fields (position, rotation, and scale) are one transform override for merge purposes, including on children. A `Drawable` is one component too, so a local tint override keeps the drawable's other local settings. The root transform is always preserved as the instance's placement; the source never receives the current instance placement. Source-added children are added to linked instances. If a source deletes a child that was locally changed, refresh stops safely and asks for the instance to be unpacked; unpack before deleting or restructuring that child locally. Hierarchy edits such as reparenting also require **Unpack** first. Linked instances cannot overlap or contain one another.

To check the linked behavior quickly, save a small hierarchy as a prefab, then use **Add to scene** twice (or drag the card twice) and move the two roots to different positions. Change a component on one instance, apply it with **Apply to prefab**, and confirm the other instance updates while both root placements remain different. Then make a source change and use **Refresh instances** to confirm the local component override remains while unchanged components update.

Prefab preparation and acceptance use the editor's asynchronous job path. Cancelled or stale work is discarded, and a scene or asset revision change during preparation prevents publication. Saving as a prefab and applying a prefab validate the complete hierarchy and its dependencies before the source write.

Prefab files can be imported through the asset browser as `.prefab.json`. Import registers a relative link to the existing prefab definition, preserving one shared source across scenes; it does not copy the definition. Importing the same prefab again in one scene reuses its catalog entry. Save As rebases the prefab link for the new scene location. No prefab definition or source dependency is copied, so keep the prefab file and its referenced image or mesh files together when sharing the project. This workflow is a source-file link and dependency rebase, not a full exporter.

[Gameplay Blueprint](blueprints.md) attachments are captured per prefab member, including their order, enabled flags, and graph data. The attachment list is one component-level override: unchanged lists receive Apply/Refresh updates, while locally edited lists are preserved. Every placed instance has independent runtime variables; graphs target their own member, not the shared mesh asset.

Nested prefabs, prefab variants, and `Player Controller` prefabs are not supported yet. A hierarchy containing a `Player Controller` must be unpacked or authored at scene level. The editor also requires Play to be stopped before prefab authoring operations.
