"""Generate two stylized static game props in Blender.

Run from the repository root:
    blender --background --python tools/generate_pistol_shotgun.py

Outputs editable .blend sources, optimized .glb files, and preview PNGs under
assets/glock18 and assets/shotgun. Dimensions are approximate visual artwork.
"""

from pathlib import Path
import math

import bpy
from mathutils import Vector


ASSETS = Path(__file__).resolve().parents[1] / "assets"
bpy.context.preferences.filepaths.save_version = 0


class Prop:
    def __init__(self, name):
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete(use_global=False)
        self.name = name
        self.root = ASSETS / name
        self.root.mkdir(parents=True, exist_ok=True)
        self.parts = []
        self.materials = []

    def mat(self, name, color, metallic=0, roughness=.5):
        mat = bpy.data.materials.new(name)
        mat.diffuse_color = (*color, 1)
        mat.use_nodes = True
        node = mat.node_tree.nodes.get("Principled BSDF")
        node.inputs["Base Color"].default_value = (*color, 1)
        node.inputs["Metallic"].default_value = metallic
        node.inputs["Roughness"].default_value = roughness
        self.materials.append(mat)
        return mat

    def finish(self, obj, name, mat, bevel=0):
        obj.name = name
        obj.data.name = name
        obj.data.materials.append(mat)
        if bevel:
            mod = obj.modifiers.new("Edge bevel", "BEVEL")
            mod.width = bevel
            mod.segments = 2
            bpy.context.view_layer.objects.active = obj
            bpy.ops.object.modifier_apply(modifier=mod.name)
        self.parts.append(obj)
        return obj

    def box(self, name, loc, size, mat, bevel=0, tilt=0):
        bpy.ops.mesh.primitive_cube_add(size=1, location=loc)
        obj = bpy.context.object
        obj.dimensions = size
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        obj.rotation_euler[1] = tilt
        return self.finish(obj, name, mat, bevel)

    def profile(self, name, outline, width, mat, bevel=0, y=0):
        n = len(outline)
        verts = [(x, y-width/2, z) for x,z in outline]
        verts += [(x, y+width/2, z) for x,z in outline]
        faces = [tuple(range(n)), tuple(reversed(range(n, 2*n)))]
        faces += [(i+n, (i+1)%n+n, (i+1)%n, i) for i in range(n)]
        mesh = bpy.data.meshes.new(name)
        mesh.from_pydata(verts, [], faces)
        mesh.update()
        obj = bpy.data.objects.new(name, mesh)
        bpy.context.collection.objects.link(obj)
        return self.finish(obj, name, mat, bevel)

    def cylinder(self, name, a, b, radius, mat, vertices=16):
        a, b = Vector(a), Vector(b)
        delta = b-a
        bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius,
                                            depth=delta.length, location=(a+b)/2)
        obj = bpy.context.object
        obj.rotation_euler = delta.to_track_quat("Z", "Y").to_euler()
        return self.finish(obj, name, mat)

    def rod(self, name, points, radius, mat):
        for i in range(len(points)-1):
            self.cylinder(f"{name} {i+1}", points[i], points[i+1], radius, mat, 8)

    def mount(self, name, loc):
        empty = bpy.data.objects.new(name, None)
        empty.empty_display_size = .015
        empty.location = loc
        bpy.context.collection.objects.link(empty)

    def output(self, camera_loc, camera_target, ortho, light_scale=1):
        # Preserve all separate parts in the editable source.
        bpy.ops.object.select_all(action="DESELECT")
        for obj in self.parts:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = self.parts[0]
        bpy.ops.wm.save_as_mainfile(filepath=str(self.root / f"{self.name}.blend"))

        # Make one exported primitive per material, which is cheaper to draw.
        export_parts = []
        for mat in self.materials:
            originals = [obj for obj in self.parts if obj.data.materials[0] == mat]
            if not originals:
                continue
            copies = []
            for obj in originals:
                clone = obj.copy()
                clone.data = obj.data.copy()
                bpy.context.collection.objects.link(clone)
                copies.append(clone)
            bpy.ops.object.select_all(action="DESELECT")
            for clone in copies:
                clone.select_set(True)
            bpy.context.view_layer.objects.active = copies[0]
            if len(copies) > 1:
                bpy.ops.object.join()
            copies[0].name = f"{self.name} {mat.name}"
            copies[0].data.name = copies[0].name
            export_parts.append(copies[0])
        bpy.ops.object.select_all(action="DESELECT")
        for obj in export_parts:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = export_parts[0]
        bpy.ops.export_scene.gltf(filepath=str(self.root / f"{self.name}.glb"),
                                  export_format="GLB", use_selection=True,
                                  export_yup=True, export_apply=True)
        for obj in export_parts:
            bpy.data.objects.remove(obj, do_unlink=True)

        # Studio preview; the camera and lights are excluded from the source/GLB.
        world = bpy.context.scene.world
        world.use_nodes = True
        world.node_tree.nodes["Background"].inputs["Color"].default_value = (.065, .085, .105, 1)
        world.node_tree.nodes["Background"].inputs["Strength"].default_value = .6
        for name, loc, energy, size in [
            ("Preview key", (.15, -.85, .85), 115, 1.2),
            ("Preview rim", (-.30, .55, .75), 125, .9),
        ]:
            data = bpy.data.lights.new(name, "AREA")
            data.energy = energy * light_scale
            data.shape = "DISK"
            data.size = size
            light = bpy.data.objects.new(name, data)
            bpy.context.collection.objects.link(light)
            light.location = loc
            light.rotation_euler = (Vector(camera_target)-light.location).to_track_quat("-Z", "Y").to_euler()
        cam_data = bpy.data.cameras.new("Preview camera")
        cam = bpy.data.objects.new("Preview camera", cam_data)
        bpy.context.collection.objects.link(cam)
        cam.location = camera_loc
        cam.rotation_euler = (Vector(camera_target)-cam.location).to_track_quat("-Z", "Y").to_euler()
        cam_data.type = "ORTHO"
        cam_data.ortho_scale = ortho
        scene = bpy.context.scene
        scene.camera = cam
        scene.render.engine = "CYCLES"
        scene.cycles.samples = 32
        scene.render.resolution_x = 1600
        scene.render.resolution_y = 900
        scene.render.resolution_percentage = 100
        scene.render.film_transparent = False
        scene.render.image_settings.file_format = "PNG"
        scene.render.filepath = str(self.root / f"{self.name}_preview.png")
        bpy.ops.render.render(write_still=True)
        print(f"Wrote {self.name} source, GLB, and preview to {self.root}")


def make_pistol():
    p = Prop("glock18")
    slide = p.mat("01 Nitrided slide", (.105, .110, .111), .75, .42)
    polymer = p.mat("02 Polymer frame", (.045, .049, .050), .05, .77)
    steel = p.mat("03 Metal details", (.23, .24, .24), .7, .44)
    recess = p.mat("04 Recesses", (.014, .017, .018), .1, .83)
    grip = p.mat("05 Grip texture", (.075, .080, .080), .05, .87)

    # Squared slide and slightly narrower lower frame.
    p.profile("Slide", [(-.110, .045), (.103, .045), (.116, .063),
                        (.116, .096), (.106, .111), (-.101, .111),
                        (-.113, .097)], .037, slide, .0025)
    p.profile("Polymer frame", [(-.101, .041), (.099, .041), (.102, .025),
                                (.059, .013), (-.046, .010), (-.071, .017),
                                (-.107, .020)], .034, polymer, .0025)
    p.box("Frame seam", (-.002, 0, .043), (.200, .038, .002), recess)
    p.box("Accessory rail", (.061, 0, .016), (.069, .038, .010), polymer, .001)
    for x in (.038, .055, .072):
        p.box("Accessory rail notch", (x, -.020, .012), (.004, .002, .006), recess, .0005)

    # A visual ejection opening, muzzle face, and slide grooves.
    p.box("Ejection port", (.027, -.020, .076), (.046, .002, .021), recess, .001)
    p.box("Ejection port metal", (.027, -.022, .067), (.035, .002, .005), steel, .0005)
    p.cylinder("Muzzle rim", (.115, 0, .074), (.118, 0, .074), .010, steel, 16)
    p.cylinder("Muzzle dark opening", (.118, 0, .074), (.119, 0, .074), .006, recess, 16)
    for side in (-1, 1):
        for i in range(7):
            x = -.091 + i*.009
            p.box(f"Rear slide serration {side} {i+1}",
                  (x, side*.019, .076), (.0023, .002, .040), recess, .0005, -.07)
        p.box(f"Frame thumb ledge {side}", (-.019, side*.019, .029),
              (.040, .003, .006), grip, .001)
    p.box("Rear sight base", (-.092, 0, .112), (.017, .032, .007), recess, .001)
    p.box("Rear sight notch", (-.092, 0, .118), (.008, .009, .006), slide, .0005)
    p.box("Front sight", (.094, 0, .115), (.010, .013, .009), recess, .001)
    p.box("Front sight dot", (.094, -.0068, .119), (.003, .001, .003), steel)

    # Angled grip, detachable extended magazine, and nonfunctional selector cue.
    p.profile("Grip", [(-.071, .014), (-.016, .014), (-.036, -.120),
                       (-.096, -.115), (-.111, -.064)], .034, polymer, .004)
    p.profile("Grip side panel", [(-.089, -.017), (-.028, -.015),
                                  (-.044, -.105), (-.092, -.101)],
              .037, grip, .0015)
    for side in (-1, 1):
        for i in range(5):
            z = -.033-i*.014
            p.box(f"Grip groove {side} {i+1}",
                  (-.058-i*.003, side*.019, z), (.044, .002, .003), recess, .0005)
    p.profile("Extended magazine", [(-.083, -.109), (-.039, -.111),
                                     (-.050, -.159), (-.090, -.157)],
              .026, slide, .0015)
    p.box("Magazine base", (-.071, 0, -.163), (.054, .031, .010), polymer, .002)
    p.rod("Trigger guard", [(-.069, -.005, .013), (-.058, -.005, -.019),
                            (.018, -.005, -.021), (.038, -.005, .018)], .003, polymer)
    p.rod("Trigger", [(-.011, -.004, .009), (-.014, -.004, -.011)], .0025, steel)
    p.cylinder("Selector boss", (-.087, -.022, .084), (-.087, -.027, .084), .006, steel, 12)
    p.box("Selector lever", (-.080, -.027, .084), (.017, .003, .004), steel, .001, .2)
    p.box("Slide release", (-.056, -.021, .039), (.021, .003, .006), steel, .0008)
    p.box("Takedown tab", (.001, -.018, .027), (.012, .004, .006), recess, .001)
    p.mount("MOUNT_muzzle_fx", (.121, 0, .074))
    p.mount("MOUNT_grip", (-.060, 0, -.050))
    p.mount("MOUNT_ejection", (.026, -.024, .079))
    p.output((.27, -.40, .22), (0, 0, -.025), .72, .30)


def make_shotgun():
    p = Prop("shotgun")
    steel = p.mat("01 Parkerized steel", (.13, .14, .14), .75, .45)
    receiver = p.mat("02 Anodized receiver", (.083, .091, .093), .70, .43)
    polymer = p.mat("03 Polymer furniture", (.049, .052, .053), .05, .77)
    edge = p.mat("04 Metal accents", (.24, .25, .25), .75, .47)
    recess = p.mat("05 Recesses", (.014, .018, .019), .08, .87)
    shell = p.mat("06 Shell hulls", (.31, .08, .065), .18, .56)

    # Receiver with a generous stock and open trigger guard.
    p.profile("Receiver", [(-.178, .053), (.128, .053), (.148, .080),
                           (.133, .151), (-.151, .150), (-.180, .126)],
              .065, receiver, .005)
    p.box("Receiver upper seam", (-.009, 0, .149), (.265, .066, .003), recess, .001)
    p.box("Ejection port", (.025, -.034, .112), (.094, .002, .034), recess, .002)
    p.box("Bolt face detail", (.027, -.036, .115), (.065, .002, .018), edge, .001)
    p.box("Ejection lip", (.027, -.039, .089), (.095, .005, .005), steel, .001)
    p.cylinder("Safety button", (-.130, -.035, .071), (-.130, -.042, .071), .007, edge, 12)
    p.cylinder("Receiver pin front", (.090, -.034, .072), (.090, -.039, .072), .005, steel, 12)
    p.cylinder("Receiver pin rear", (-.112, -.034, .072), (-.112, -.039, .072), .005, steel, 12)
    p.rod("Trigger guard", [(-.070, -.006, .051), (-.050, -.006, .018),
                            (.034, -.006, .018), (.054, -.006, .055)], .004, steel)
    p.rod("Trigger", [(-.006, -.006, .050), (-.004, -.006, .027)], .003, edge)

    # Tapered stock with a sculpted lower edge and soft butt pad.
    p.profile("Shoulder stock", [(-.522, .179), (-.403, .165), (-.313, .139),
                                 (-.195, .120), (-.189, .073), (-.250, .042),
                                 (-.327, -.022), (-.367, -.071), (-.419, -.050),
                                 (-.445, .050), (-.524, .044)],
              .069, polymer, .009)
    p.profile("Stock cheek line", [(-.494, .173), (-.398, .162), (-.306, .135),
                                  (-.322, .125), (-.438, .153)],
              .073, receiver, .002)
    p.box("Butt pad", (-.533, 0, .108), (.019, .084, .143), polymer, .005, -.04)
    p.box("Stock grip texture", (-.371, -.036, -.006), (.053, .002, .077), receiver, .004, .16)
    for i in range(4):
        p.box(f"Stock grip line {i+1}", (-.379+i*.009, -.039, -.036+i*.013),
              (.034, .002, .004), recess, .0005, .16)
    p.box("Stock sling socket", (-.477, -.038, .067), (.022, .004, .012), edge, .002)

    # Barrel above the magazine tube. Everything is solid visual geometry.
    p.cylinder("Shotgun barrel", (.128, 0, .119), (.672, 0, .119), .021, steel, 20)
    p.cylinder("Barrel shoulder", (.127, 0, .119), (.155, 0, .119), .026, edge, 16)
    p.cylinder("Muzzle rim", (.668, 0, .119), (.679, 0, .119), .023, edge, 20)
    p.cylinder("Muzzle dark face", (.680, 0, .119), (.681, 0, .119), .014, recess, 20)
    p.cylinder("Magazine tube", (.124, 0, .066), (.616, 0, .066), .015, steel, 16)
    p.cylinder("Magazine end cap", (.603, 0, .066), (.632, 0, .066), .018, edge, 16)
    p.box("Front barrel clamp", (.567, 0, .091), (.018, .042, .066), receiver, .004)
    p.box("Vent rib", (.390, 0, .142), (.475, .011, .005), edge, .001)
    for i in range(6):
        x = .197+i*.066
        p.box(f"Rib standoff {i+1}", (x, 0, .134), (.005, .012, .014), recess, .001)
    p.box("Front bead base", (.634, 0, .147), (.016, .013, .009), steel, .001)
    p.cylinder("Front bead", (.634, 0, .152), (.634, 0, .159), .004, edge, 12)

    # Sliding pump forend, ribbed so it reads at camera distance.
    p.profile("Pump forend", [(.167, .101), (.424, .101), (.438, .085),
                              (.438, .041), (.419, .028), (.181, .028),
                              (.164, .042)], .070, polymer, .008)
    p.box("Forend top channel", (.301, 0, .102), (.236, .050, .004), recess, .001)
    for side in (-1, 1):
        for i in range(10):
            x = .193+i*.024
            p.box(f"Pump rib {side} {i+1}", (x, side*.036, .064),
                  (.008, .003, .045), receiver, .001)
    p.cylinder("Forend front band", (.434, 0, .066), (.448, 0, .066), .028, edge, 16)

    # Side saddle with three shells gives the silhouette a clear game prop cue.
    p.box("Shell carrier", (-.084, -.039, .103), (.122, .009, .058), polymer, .003)
    for i, x in enumerate((-.124, -.085, -.046)):
        p.cylinder(f"Shell {i+1} hull", (x, -.049, .077), (x, -.049, .126), .011, shell, 12)
        p.cylinder(f"Shell {i+1} head", (x, -.049, .074), (x, -.049, .080), .012, edge, 12)
    p.mount("MOUNT_muzzle_fx", (.684, 0, .119))
    p.mount("MOUNT_grip", (-.365, 0, -.017))
    p.mount("MOUNT_ejection", (.027, -.041, .112))
    p.output((1.03, -1.32, .63), (.04, 0, .055), 1.50, .65)


make_pistol()
make_shotgun()
