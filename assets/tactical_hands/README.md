# Tactical first-person hands

Two gloved hands and forearms in a neutral two-hand weapon hold. The pair is
positioned around the same **+X** weapon axis as the AR-15, pistol, and shotgun
props. Fine placement and animation should be tuned for each weapon later.

- `tactical_hands.blend`: editable source with separate parts and an armature.
- `tactical_hands.glb`: compact static pair for quick preview.
- `tactical_hands_rigged.glb`: both hands with named wrist and finger bones.
- `hand_R.glb` and `hand_L.glb`: individual static hands with origins at their
  wrist pivots, for independent placement.
- `tactical_hands_preview.png`: studio preview.

Blender source is Z-up; GLBs are Y-up. Units are meters. These are art assets,
without weapon animations, collision meshes, or gameplay logic. Regenerate from
the repository root:

```sh
blender --background --python tools/generate_character_assets.py
```
