# Gilded Hour armillary

`armillary-ring.gltf` and `armillary-filament.gltf`, with their `.bin` buffers,
are original procedural torus meshes created for Bozzard's Sponza showcase.
They contain geometry, normals, UVs and a satin bronze material; no downloaded
model or texture content is included.

These four generated asset files are dedicated to the public domain under
**CC0 1.0 Universal**: <https://creativecommons.org/publicdomain/zero/1.0/>.

Reproduce or verify them from the repository root:

```sh
python3 tools/gen_sponza_showcase_assets.py
python3 tools/gen_sponza_showcase_assets.py --check
```

This dedication applies only to these original armillary assets. The Sponza
architecture and textures have separate upstream terms; see
[Sponza source and licensing](../../../../docs/sponza.md).
