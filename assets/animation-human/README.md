# Animation lab characters

These are original procedural Blender characters created for Bozzard. They contain no third-party models, textures, rigs or motion-capture data.

| File | Purpose |
| --- | --- |
| `human.blend` | Editable blue-jacket character, skeleton and actions |
| `human.glb` | One skin, eighteen deforming bones, six materials and ten clips |
| `human-tall.blend` | Editable taller orange-jacket character with different proportions |
| `human-tall.glb` | Skeleton and mesh without clips; the engine retargets the first character's motion |

The clips are `Idle`, `WalkForward`, `WalkBack`, `WalkLeft`, `WalkRight`, `RunForward`, `Wave`, `Aim`, `Jump` and `Reach`. Walking directions share a one-second cycle. Forward running has a shorter cycle; the engine blends normalized phase across the different durations. `Reach` has root travel for motion warping.

The torso has blended bone weights. The separate stylized limb shapes use rigid weights with overlapping joints. These assets demonstrate engine behavior; they are not realistic anatomical deformation or motion capture.

Rebuild both Blender sources and GLBs with Blender 5.2 or newer:

```sh
blender --background --factory-startup --python tools/generate_animation_character.py
cargo run --release -p bozzard-editor --example animation_lab
```

The exporter converts Blender's Z-up coordinates into glTF coordinates. The engine keeps the imported bone bases when extracting root motion. The generated scene embeds cooked clips, including the taller character's baked retargeted clips, and refers to these GLBs for mesh geometry.
