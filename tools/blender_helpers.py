"""Small shared helpers for the Blender asset generation scripts.

This module deliberately accepts Blender's ``bpy`` module as an argument so
its geometry and material setup can be checked without launching Blender.
"""


def create_principled_material(bpy, name, color, metallic=0.0, roughness=0.5):
    """Create the simple Principled material used by generated assets."""
    material = bpy.data.materials.new(name)
    material.diffuse_color = (*color, 1)
    material.use_nodes = True
    shader = material.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value = (*color, 1)
    shader.inputs["Metallic"].default_value = metallic
    shader.inputs["Roughness"].default_value = roughness
    return material


def create_extruded_profile(bpy, name, outline, width, y=0):
    """Create a closed mesh by extruding an X/Z outline along the Y axis."""
    count = len(outline)
    vertices = [(x, y - width / 2, z) for x, z in outline]
    vertices += [(x, y + width / 2, z) for x, z in outline]
    faces = [tuple(range(count)), tuple(reversed(range(count, 2 * count)))]
    faces += [
        (i + count, (i + 1) % count + count, (i + 1) % count, i)
        for i in range(count)
    ]

    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return obj
