"""Add the supplied dog artwork as a framed canvas, with its texture packed."""
import bpy
import os


def add_portrait(directory):
    # Replace the small music poster in the original room.
    for name in ['Music print frame', 'Music print paper', 'Poster graphic sun', 'Poster headline']:
        obj = bpy.data.objects.get(name)
        if obj:
            bpy.data.objects.remove(obj, do_unlink=True)
    existing = bpy.data.collections.get('Framed dog painting')
    if existing:
        for obj in list(existing.objects):
            bpy.data.objects.remove(obj, do_unlink=True)
        bpy.data.collections.remove(existing)
    collection = bpy.data.collections.new('Framed dog painting')
    bpy.context.scene.collection.children.link(collection)

    def box(name, location, dimensions, material, bevel):
        bpy.ops.mesh.primitive_cube_add(size=1, location=location)
        obj = bpy.context.object
        obj.name = name
        for c in list(obj.users_collection):
            c.objects.unlink(obj)
        collection.objects.link(obj)
        obj.dimensions = dimensions
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        obj.data.materials.append(material)
        mod = obj.modifiers.new('Frame edge softness', 'BEVEL')
        mod.width = bevel
        mod.segments = 3
        obj.modifiers.new('Frame normals', 'WEIGHTED_NORMAL')
        return obj

    ivory = bpy.data.materials['Warm ivory trim']
    wood = bpy.data.materials['Walnut']
    oak = bpy.data.materials['Honey oak']
    cy, cz = -1.66, 2.08
    box('Portrait walnut frame backing', (-2.649, cy, cz), (.09, .79, 1.02), wood, .014)
    box('Portrait ivory mount', (-2.592, cy, cz), (.023, .714, .944), ivory, .005)
    for y in [cy-.382, cy+.382]:
        box('Portrait frame vertical rail', (-2.589, y, cz), (.043, .027, 1.00), oak, .006)
    for z in [cz-.497, cz+.497]:
        box('Portrait frame horizontal rail', (-2.589, cy, z), (.043, .79, .027), oak, .006)

    # Preserve the source image and its aspect ratio; its alpha reveals canvas.
    image = bpy.data.images.load(os.path.join(directory, 'dog_portrait.png'), check_existing=True)
    image.name = 'Supplied dog portrait — packed original'
    image.pack()
    width = .631
    height = width * image.size[1] / image.size[0]
    x = -2.575
    mesh = bpy.data.meshes.new('Portrait canvas UV mesh')
    mesh.from_pydata([(x,cy-width/2,cz-height/2), (x,cy+width/2,cz-height/2),
                     (x,cy+width/2,cz+height/2), (x,cy-width/2,cz+height/2)], [], [(0,1,2,3)])
    mesh.update()
    uv = mesh.uv_layers.new(name='Artwork UV')
    for i, co in enumerate([(0,0),(1,0),(1,1),(0,1)]):
        uv.data[i].uv = co
    obj = bpy.data.objects.new('Dog portrait on textured canvas', mesh)
    collection.objects.link(obj)
    material = bpy.data.materials.new('Dog portrait · pigment on ivory canvas')
    material.use_nodes = True
    n, l = material.node_tree.nodes, material.node_tree.links
    p = n.get('Principled BSDF')
    p.inputs['Roughness'].default_value = .87
    tex = n.new('ShaderNodeTexImage')
    tex.image = image
    tex.interpolation = 'Linear'
    mix = n.new('ShaderNodeMixRGB')
    mix.blend_type = 'MIX'
    mix.inputs[1].default_value = (.84, .79, .66, 1)
    l.new(tex.outputs['Alpha'], mix.inputs[0])
    l.new(tex.outputs['Color'], mix.inputs[2])
    l.new(mix.outputs[0], p.inputs['Base Color'])
    noise = n.new('ShaderNodeTexNoise')
    noise.inputs['Scale'].default_value = 220
    bump = n.new('ShaderNodeBump')
    bump.inputs['Strength'].default_value = .15
    bump.inputs['Distance'].default_value = .006
    l.new(noise.outputs['Fac'], bump.inputs['Height'])
    l.new(bump.outputs['Normal'], p.inputs['Normal'])
    mesh.materials.append(material)
    obj['Artwork'] = 'User-supplied dog portrait; original image embedded in blend file'
    bpy.ops.object.select_all(action='DESELECT')
    return obj


def add_photo(directory):
    """Hang the second supplied picture below the neon, above the headboard."""
    name = 'Framed dog photograph'
    old = bpy.data.collections.get(name)
    if old:
        for obj in list(old.objects):
            bpy.data.objects.remove(obj, do_unlink=True)
        bpy.data.collections.remove(old)
    collection = bpy.data.collections.new(name)
    bpy.context.scene.collection.children.link(collection)
    cx, cz = -1.69, 1.665

    def box(name, location, size, material):
        bpy.ops.mesh.primitive_cube_add(size=1, location=location)
        obj = bpy.context.object
        obj.name = name
        for c in list(obj.users_collection):
            c.objects.unlink(obj)
        collection.objects.link(obj)
        obj.dimensions = size
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        obj.data.materials.append(bpy.data.materials[material])
        mod = obj.modifiers.new('Soft frame edges', 'BEVEL')
        mod.width = .006
        mod.segments = 3
        obj.modifiers.new('Frame normals', 'WEIGHTED_NORMAL')

    box('Photo walnut backing', (cx,2.207,cz), (.50,.06,.66), 'Walnut')
    box('Photo ivory mount', (cx,2.166,cz), (.468,.025,.628), 'Warm ivory trim')
    for x in [cx-.24,cx+.24]:
        box('Photo oak side rail', (x,2.15,cz), (.020,.04,.66), 'Honey oak')
    for z in [cz-.32,cz+.32]:
        box('Photo oak horizontal rail', (cx,2.15,z), (.50,.04,.020), 'Honey oak')
    image = bpy.data.images.load(os.path.join(directory,'dog_photo.png'), check_existing=True)
    image.name = 'Second supplied dog photograph — packed original'
    image.pack()
    w = .4
    h = w * image.size[1] / image.size[0]
    y = 2.147
    mesh = bpy.data.meshes.new('Second dog picture UV mesh')
    mesh.from_pydata([(cx-w/2,y,cz-h/2),(cx+w/2,y,cz-h/2),
                     (cx+w/2,y,cz+h/2),(cx-w/2,y,cz+h/2)],[],[(0,1,2,3)])
    mesh.update()
    uv = mesh.uv_layers.new(name='Original photo UV')
    for i,co in enumerate([(0,0),(1,0),(1,1),(0,1)]):
        uv.data[i].uv = co
    obj = bpy.data.objects.new('Second dog photograph',mesh)
    collection.objects.link(obj)
    material = bpy.data.materials.new('Second dog photograph · matte print')
    material.use_nodes = True
    p = material.node_tree.nodes.get('Principled BSDF')
    p.inputs['Roughness'].default_value = .8
    tex = material.node_tree.nodes.new('ShaderNodeTexImage')
    tex.image = image
    material.node_tree.links.new(tex.outputs['Color'],p.inputs['Base Color'])
    mesh.materials.append(material)
    obj['Artwork'] = 'Second user-supplied dog photo, uncropped; embedded in blend file'
    bpy.ops.object.select_all(action='DESELECT')


if __name__ == '__main__':
    directory = os.path.dirname(os.path.abspath(__file__))
    add_portrait(directory)
    add_photo(directory)
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(directory, 'bozzard_dorm.blend'))
    bpy.ops.render.render(write_still=True)
    print('PORTRAIT_COMPLETE', flush=True)
