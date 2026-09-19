# FBX conversion

Bozzard accepts glTF 2.0/GLB and OBJ. Convert FBX through Blender before importing;
FBX files are not passed to an engine-side FBX decoder.

1. In a fresh Blender scene, remove the default objects and use **File → Import →
   FBX**. Check orientation, scale, material textures, and animation playback in
   Blender before exporting. Keep the original FBX and source textures.
2. Prepare ordinary metallic/roughness materials. Bake procedural Blender material
   effects into PNG/JPEG textures. Match the intended unit scale and object origins;
   LOD replacements must use the same origin and scale as their base mesh.
3. Use **File → Export → glTF 2.0** and choose **glTF Binary (.glb)** for one portable
   file, or **glTF Separate** for large assets. Export triangle meshes, normals,
   UVs, materials, and +Y up. Leave Draco/meshopt compression, GPU instancing,
   WebP/BasisU texture extensions, and extra material extensions disabled.
4. For skeletal animation, export skins with at most four joint influences per
   vertex. Bake constraints into sampled translation/rotation/scale animation.
   Review the selected actions/clips in the exported result. Do not export shape
   keys/morph animation: Bozzard currently rejects morph targets. Disable vertex
   colors unless their appearance has been baked into the base-color texture.
5. In Bozzard, **File → Import asset…**, select the `.glb`/`.gltf`, and add it to the
   scene. The importer checks the actual file and reports unsupported content.
   It copies a glTF package and its dependencies into the scene's asset directory.

Blender's exporter exposes the relevant controls as `export_yup`, `export_skins`,
`export_influence_nb`, `export_all_influences`, `export_force_sampling`, and
`export_morph`; see the [official exporter reference](https://docs.blender.org/api/main/bpy.ops.export_scene.html).
Names and arrangement of UI panels vary between Blender releases. The documented
FBX import menu and export settings were checked against the [official Blender FBX
add-on source](https://github.com/blender/blender-addons/blob/main/io_scene_fbx/__init__.py)
and [Khronos glTF exporter source](https://github.com/KhronosGroup/glTF-Blender-IO/blob/main/addons/io_scene_gltf2/__init__.py).
Blender was not installed on the verification host, so this is a documented
conversion path, not a claim that an FBX conversion was executed there.

Before accepting a converted asset, compare its silhouettes, normals, materials,
scale, and animations to the original in several poses. Import once as a GLB,
save the scene, close/reopen it, and export the project. Test the exported game
from a different directory with the original FBX unavailable. A successful file
conversion alone does not establish visual fidelity. This documented workflow
uses Bozzard's existing glTF validation and export paths; Blender is an authoring
dependency, not a player/runtime dependency.

Current engine limits and supported material data are in [assets](assets.md).
Skinning, animation authoring and limits are in [middleware](middleware.md).
