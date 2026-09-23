# Pump-action shotgun game prop

Stylized, static visual model with a ribbed pump forend, tubular magazine,
shoulder stock, and side shell carrier. Proportions are approximate game artwork.

- `shotgun.blend`: editable Blender source with named parts and mount empties.
- `shotgun.glb`: engine asset, joined into six PBR material groups.
- `shotgun_preview.png`: studio preview.

The muzzle points along **+X**. Blender's Z-up source is exported to glTF's
Y-up system. Units are meters. The asset has no rig, animations, collision mesh,
or gameplay logic.

In Bozzard, use **File → Import asset…** and select `shotgun.glb`.
Regenerate from the repository root with:

```sh
blender --background --python tools/generate_pistol_shotgun.py
```
