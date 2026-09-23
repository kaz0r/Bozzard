"""Export the authored room as portable glTF with baked procedural textures.

blender -b assets/dorm_room/bozzard_dorm.blend --python assets/dorm_room/export_room.py
The source .blend is never overwritten. Studio ground, cameras and lights are
excluded; the engine scene supplies its own lighting, camera and HDR bloom.
"""
import bpy
import os
import numpy as np

OUT = os.path.dirname(os.path.abspath(__file__))
scene = bpy.context.scene
scene.render.engine = 'CYCLES'
scene.cycles.samples = 1
scene.cycles.device = 'CPU'
scene.render.threads_mode = 'FIXED'
scene.render.threads = 8
scene.render.bake.margin = 12
scene.render.bake.use_clear = True
depsgraph = bpy.context.evaluated_depsgraph_get()
originals = list(scene.objects)
parts = []
for obj in originals:
    if obj.type not in {'MESH', 'CURVE', 'FONT'} or obj.name == 'Studio ground':
        continue
    mesh = bpy.data.meshes.new_from_object(obj.evaluated_get(depsgraph),
                preserve_all_data_layers=True, depsgraph=depsgraph)
    if not mesh.vertices:
        bpy.data.meshes.remove(mesh)
        continue
    coordinates = np.empty(len(mesh.vertices)*3, dtype=np.float32)
    mesh.vertices.foreach_get('co',coordinates)
    coordinates = coordinates.reshape(-1,3)
    low = coordinates.min(axis=0)
    size = np.maximum(coordinates.max(axis=0)-low, .00001)
    generated = mesh.attributes.new('OriginalGenerated','FLOAT_VECTOR','POINT')
    generated.data.foreach_set('vector',((coordinates-low)/size).ravel())
    if mesh.uv_layers:
        mesh.uv_layers[0].name = 'SourceUV'
    else:
        mesh.uv_layers.new(name='SourceUV')
    mesh.transform(obj.matrix_world)
    part = bpy.data.objects.new(obj.name+' export',mesh)
    scene.collection.objects.link(part)
    parts.append(part)
for obj in originals:
    bpy.data.objects.remove(obj,do_unlink=True)
bpy.ops.object.select_all(action='DESELECT')
for obj in parts:
    obj.select_set(True)
bpy.context.view_layer.objects.active=parts[0]
bpy.ops.object.join()
room=bpy.context.object
room.name='Bozzard dorm room — Room 207'
print('JOINED',len(room.data.vertices),'vertices',len(room.data.polygons),'faces',flush=True)

# Freeze each source object's Generated coordinates and source photo UVs before
# packing a separate non-overlapping atlas for portable base color and normals.
materials=list({s.material.name:s.material for s in room.material_slots if s.material}.values())
saved=[]
photo_sources={}
for material in materials:
    n,l=material.node_tree.nodes,material.node_tree.links
    p=n.get('Principled BSDF')
    if material.name.startswith(('Dog portrait', 'Second dog photograph')):
        photo_sources[material.name]=next(node.image for node in n if node.type=='TEX_IMAGE' and node.image)
    attr=n.new('ShaderNodeAttribute');attr.attribute_name='OriginalGenerated'
    uv=n.new('ShaderNodeUVMap');uv.uv_map='SourceUV'
    for node in list(n):
        if node.type=='TEX_COORD':
            for link in list(node.outputs['Generated'].links):
                l.new(attr.outputs['Vector'],link.to_socket)
        if node.type=='TEX_NOISE' and not node.inputs['Vector'].is_linked:
            l.new(attr.outputs['Vector'],node.inputs['Vector'])
        if node.type=='TEX_IMAGE' and not node.inputs['Vector'].is_linked:
            l.new(uv.outputs['UV'],node.inputs['Vector'])
    saved.append((material,p.inputs['Metallic'].default_value,p.inputs['Roughness'].default_value,
                  tuple(p.inputs['Emission Color'].default_value),p.inputs['Emission Strength'].default_value,
                  p.inputs['Normal'].is_linked))

atlas=room.data.uv_layers.new(name='RoomAtlas')
room.data.uv_layers.active=atlas
atlas.active_render=True
bpy.ops.object.mode_set(mode='EDIT')
bpy.ops.mesh.select_all(action='SELECT')
bpy.ops.uv.smart_project(angle_limit=1.15192,island_margin=.0018,area_weight=.2,correct_aspect=True)
bpy.ops.object.mode_set(mode='OBJECT')
base=bpy.data.images.new('dorm_basecolor',width=4096,height=4096,alpha=False)
base.colorspace_settings.name='sRGB'
normal=bpy.data.images.new('dorm_normal',width=4096,height=4096,alpha=False)
normal.colorspace_settings.name='Non-Color'
targets=[]
for material,*_ in saved:
    n,l=material.node_tree.nodes,material.node_tree.links
    p=n.get('Principled BSDF');out=n.get('Material Output')
    target=n.new('ShaderNodeTexImage');target.image=base;n.active=target;targets.append(target)
    emission=n.new('ShaderNodeEmission')
    color=p.inputs['Base Color']
    if color.is_linked:l.new(color.links[0].from_socket,emission.inputs['Color'])
    else:emission.inputs['Color'].default_value=color.default_value
    l.new(emission.outputs[0],out.inputs['Surface'])
print('BAKING BASE COLOR',flush=True)
bpy.ops.object.bake(type='EMIT')
base.filepath_raw=os.path.join(OUT,'dorm_basecolor.png');base.file_format='PNG';base.save()
for (material,*_),target in zip(saved,targets):
    material.node_tree.links.new(material.node_tree.nodes.get('Principled BSDF').outputs[0],
                                 material.node_tree.nodes.get('Material Output').inputs['Surface'])
    target.image=normal
print('BAKING NORMALS',flush=True)
bpy.ops.object.bake(type='NORMAL')
normal.filepath_raw=os.path.join(OUT,'dorm_normal.png');normal.file_format='PNG';normal.save()

# Simple glTF-compatible material graphs, retaining HDR emission on neon glass.
for material,metal,rough,emission,strength,has_bump in saved:
    n,l=material.node_tree.nodes,material.node_tree.links
    n.clear()
    p=n.new('ShaderNodeBsdfPrincipled');out=n.new('ShaderNodeOutputMaterial')
    p.inputs['Metallic'].default_value=metal;p.inputs['Roughness'].default_value=rough
    p.inputs['Emission Color'].default_value=emission;p.inputs['Emission Strength'].default_value=strength
    photo=photo_sources.get(material.name)
    uv=n.new('ShaderNodeUVMap');uv.uv_map='SourceUV' if photo else 'RoomAtlas'
    tex=n.new('ShaderNodeTexImage');tex.image=photo or base
    l.new(uv.outputs[0],tex.inputs['Vector']);l.new(tex.outputs['Color'],p.inputs['Base Color'])
    if photo and material.name.startswith('Dog portrait'):
        l.new(tex.outputs['Alpha'],p.inputs['Alpha'])
    if has_bump and not photo:
        texn=n.new('ShaderNodeTexImage');texn.image=normal
        nm=n.new('ShaderNodeNormalMap');nm.uv_map='RoomAtlas'
        l.new(uv.outputs[0],texn.inputs['Vector']);l.new(texn.outputs['Color'],nm.inputs['Color'])
        l.new(nm.outputs[0],p.inputs['Normal'])
    l.new(p.outputs[0],out.inputs[0])
    material.use_backface_culling=False
room.data.uv_layers.active_index=1
bpy.ops.export_scene.gltf(filepath=os.path.join(OUT,'bozzard_dorm.gltf'),
    export_format='GLTF_SEPARATE',use_selection=True,export_yup=True,
    export_apply=True,export_animations=False,export_cameras=False,export_lights=False,
    export_extras=False,export_image_format='AUTO')
bpy.ops.export_scene.gltf(filepath=os.path.join(OUT,'bozzard_dorm.glb'),
    export_format='GLB',use_selection=True,export_yup=True,
    export_apply=True,export_animations=False,export_cameras=False,export_lights=False,
    export_extras=False,export_image_format='AUTO')
print('ROOM_EXPORT_COMPLETE',flush=True)
