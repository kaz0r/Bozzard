# Glock 18 game prop

Stylized, static visual model with a squared slide, polymer frame, rear selector
cue, and extended magazine. Proportions are approximate game artwork.

- `glock18.blend`: editable Blender source with named parts and mount empties.
- `glock18.glb`: engine asset, joined into five PBR material groups.
- `glock18_preview.png`: studio preview.

The muzzle points along **+X**. Blender's Z-up source is exported to glTF's
Y-up system. Units are meters. The asset has no rig, animations, collision mesh,
or gameplay logic.

In Bozzard, use **File → Import asset…** and select `glock18.glb`.
Regenerate from the repository root with:

```sh
blender --background --python tools/generate_pistol_shotgun.py
```
