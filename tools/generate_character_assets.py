"""Create first-person tactical hands and an articulated demo mannequin.

Run: blender --background --python tools/generate_character_assets.py
The static GLBs are optimized for Bozzard's ordinary Drawable component.
Rigged GLBs and editable Blender sources retain named bones for future posing.
"""

from pathlib import Path
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from blender_helpers import create_extruded_profile, create_principled_material


ROOT = Path(__file__).resolve().parents[1] / "assets"
bpy.context.preferences.filepaths.save_version = 0


class Character:
    def __init__(self, name):
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete(use_global=False)
        self.name = name
        self.root = ROOT / name
        self.root.mkdir(parents=True, exist_ok=True)
        self.parts = []
        self.bindings = {}
        self.materials = []
        self.armature = None

    def mat(self, name, color, metallic=0, roughness=.65):
        material = create_principled_material(bpy, name, color, metallic, roughness)
        self.materials.append(material)
        return material

    def finish(self, obj, name, material, bone, bevel=0):
        obj.name = name
        obj.data.name = name
        obj.data.materials.append(material)
        if bevel:
            mod = obj.modifiers.new("Soft edges", "BEVEL")
            mod.width = bevel
            mod.segments = 2
            bpy.context.view_layer.objects.active = obj
            bpy.ops.object.modifier_apply(modifier=mod.name)
        self.parts.append(obj)
        self.bindings[obj] = bone
        return obj

    def box(self, name, loc, size, material, bone, bevel=0):
        bpy.ops.mesh.primitive_cube_add(size=1, location=loc)
        obj = bpy.context.object
        obj.dimensions = size
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        return self.finish(obj, name, material, bone, bevel)

    def ellipsoid(self, name, loc, size, material, bone, segments=12):
        bpy.ops.mesh.primitive_uv_sphere_add(segments=segments, ring_count=8,
                                             radius=1, location=loc)
        obj = bpy.context.object
        obj.scale = size
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        return self.finish(obj, name, material, bone)

    def segment(self, name, a, b, radius_a, radius_b, material, bone, vertices=12):
        a, b = Vector(a), Vector(b)
        delta = b-a
        bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=radius_a,
                                        radius2=radius_b, depth=delta.length,
                                        location=(a+b)/2)
        obj = bpy.context.object
        obj.rotation_euler = delta.to_track_quat("Z", "Y").to_euler()
        return self.finish(obj, name, material, bone)

    def profile(self, name, outline, width, material, bone, bevel=0, y=0):
        obj = create_extruded_profile(bpy, name, outline, width, y)
        return self.finish(obj, name, material, bone, bevel)

    def rig(self, bones):
        data = bpy.data.armatures.new(f"{self.name} armature")
        rig = bpy.data.objects.new(f"{self.name} Rig", data)
        bpy.context.collection.objects.link(rig)
        bpy.context.view_layer.objects.active = rig
        rig.select_set(True)
        bpy.ops.object.mode_set(mode="EDIT")
        for name, head, tail, parent in bones:
            bone = data.edit_bones.new(name)
            bone.head = head
            bone.tail = tail
            if parent:
                bone.parent = data.edit_bones[parent]
                bone.use_connect = False
        bpy.ops.object.mode_set(mode="OBJECT")
        rig.show_in_front = True
        self.armature = rig
        for obj in self.parts:
            group = obj.vertex_groups.new(name=self.bindings[obj])
            group.add(list(range(len(obj.data.vertices))), 1.0, "REPLACE")
            mod = obj.modifiers.new("Rigid segment skin", "ARMATURE")
            mod.object = rig
            obj.parent = rig
        return rig

    def export_subset(self, side, pivot):
        """Export one static hand with its wrist at the GLB origin."""
        selected = [obj for obj in self.parts if obj.name.startswith(f"{side} ")]
        joined = []
        for mat in self.materials:
            originals = [obj for obj in selected if obj.data.materials[0] == mat]
            if not originals:
                continue
            copies = []
            for original in originals:
                clone = original.copy()
                clone.data = original.data.copy()
                bpy.context.collection.objects.link(clone)
                clone.parent = None
                for mod in list(clone.modifiers):
                    if mod.type == "ARMATURE":
                        clone.modifiers.remove(mod)
                transform = original.matrix_world.copy()
                transform.translation -= Vector(pivot)
                clone.matrix_world = transform
                copies.append(clone)
            bpy.ops.object.select_all(action="DESELECT")
            for clone in copies:
                clone.select_set(True)
            bpy.context.view_layer.objects.active = copies[0]
            if len(copies) > 1:
                bpy.ops.object.join()
            copies[0].name = f"hand_{side} {mat.name}"
            copies[0].data.name = copies[0].name
            joined.append(copies[0])
        bpy.ops.object.select_all(action="DESELECT")
        for obj in joined:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = joined[0]
        bpy.ops.export_scene.gltf(filepath=str(self.root / f"hand_{side}.glb"),
                                  export_format="GLB", use_selection=True,
                                  export_yup=True, export_apply=True)
        for obj in joined:
            bpy.data.objects.remove(obj, do_unlink=True)

    def output(self, camera_loc, camera_target, ortho, light_scale=1):
        # Save editable source, including all distinct parts and armature bones.
        bpy.ops.object.select_all(action="DESELECT")
        self.armature.select_set(True)
        for obj in self.parts:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = self.armature
        bpy.ops.wm.save_as_mainfile(filepath=str(self.root / f"{self.name}.blend"))

        # Export a skinned version for future animation/attachment workflows.
        bpy.ops.export_scene.gltf(filepath=str(self.root / f"{self.name}_rigged.glb"),
                                  export_format="GLB", use_selection=True,
                                  export_yup=True, export_skins=True,
                                  export_animations=False)

        # The demo Drawable uses a compact static mesh, grouped by material.
        joined = []
        for mat in self.materials:
            originals = [obj for obj in self.parts if obj.data.materials[0] == mat]
            if not originals:
                continue
            copies = []
            for original in originals:
                clone = original.copy()
                clone.data = original.data.copy()
                bpy.context.collection.objects.link(clone)
                clone.parent = None
                for mod in list(clone.modifiers):
                    if mod.type == "ARMATURE":
                        clone.modifiers.remove(mod)
                copies.append(clone)
            bpy.ops.object.select_all(action="DESELECT")
            for clone in copies:
                clone.select_set(True)
            bpy.context.view_layer.objects.active = copies[0]
            if len(copies) > 1:
                bpy.ops.object.join()
            copies[0].name = f"{self.name} {mat.name}"
            copies[0].data.name = copies[0].name
            joined.append(copies[0])
        bpy.ops.object.select_all(action="DESELECT")
        for obj in joined:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = joined[0]
        bpy.ops.export_scene.gltf(filepath=str(self.root / f"{self.name}.glb"),
                                  export_format="GLB", use_selection=True,
                                  export_yup=True, export_apply=True)
        for obj in joined:
            bpy.data.objects.remove(obj, do_unlink=True)

        # Preview does not add studio objects to either GLB or source .blend.
        self.armature.hide_render = True
        world = bpy.context.scene.world
        world.use_nodes = True
        world.node_tree.nodes["Background"].inputs["Color"].default_value = (.060, .077, .096, 1)
        world.node_tree.nodes["Background"].inputs["Strength"].default_value = .62
        for name, loc, power, size in [
            ("Preview key", (.6, -1.4, 2.7), 260, 2.2),
            ("Preview fill", (-.9, .9, 2.2), 180, 1.8),
        ]:
            data = bpy.data.lights.new(name, "AREA")
            data.energy = power * light_scale
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
        print(f"Wrote {self.name} source, static/rigged GLBs, and preview")


def make_hands():
    c = Character("tactical_hands")
    fabric = c.mat("01 Olive tactical fabric", (.078, .105, .083), 0, .91)
    rubber = c.mat("02 Charcoal rubber", (.034, .040, .039), .03, .76)
    armor = c.mat("03 Knuckle armor", (.18, .20, .19), .28, .58)
    seam = c.mat("04 Reinforced stitching", (.25, .28, .23), 0, .92)

    # Primary hand is lower/rear; support hand rests further forward. The pose
    # roughly matches the shared +X weapon axis without being fused to a gun.
    specs = [
        ("R", (-.285, -.48, -.31), (-.120, -.125, -.135),
         (-.105, -.065, -.088), -.17, (-.055, -.073, -.120)),
        ("L", (.455, -.48, -.30), (.305, -.135, -.055),
         (.286, -.066, .015), -.035, (.219, -.073, .021)),
    ]
    bones = [("Root", (0, 0, -.45), (0, 0, -.30), None)]
    for side, elbow, wrist, palm, finger_z, thumb in specs:
        fore = f"{side}_Forearm"
        hand = f"{side}_Hand"
        bones.append((fore, elbow, wrist, "Root"))
        bones.append((hand, wrist, palm, fore))
        for index in range(4):
            suffix = ["Index", "Middle", "Ring", "Pinky"][index]
            bone = f"{side}_{suffix}"
            shift = (index-1.5)*.019
            bones.append((bone, (palm[0]+shift, palm[1], palm[2]-.018),
                          (palm[0]+shift, palm[1], finger_z), hand))
        bones.append((f"{side}_Thumb", palm, thumb, hand))

        # Sleeved forearm, hard cuff, glove palm and knuckle guard.
        c.segment(f"{side} sleeve", elbow, wrist, .075, .046, fabric, fore)
        c.segment(f"{side} sleeve panel", (elbow[0], elbow[1]-.025, elbow[2]+.018),
                  (wrist[0], wrist[1]-.014, wrist[2]+.010), .042, .030, rubber, fore)
        c.ellipsoid(f"{side} elbow cap", elbow, (.067, .065, .054), rubber, fore)
        c.segment(f"{side} wrist cuff", (wrist[0]-.006, wrist[1]-.012, wrist[2]-.025),
                  (wrist[0]+.006, wrist[1]+.012, wrist[2]+.022),
                  .054, .054, rubber, hand)
        c.segment(f"{side} glove wrist bridge", wrist, palm,
                  .043, .041, fabric, hand)
        c.ellipsoid(f"{side} palm", palm, (.059, .040, .047), fabric, hand)
        c.box(f"{side} hand back plate", (palm[0], palm[1]-.036, palm[2]+.016),
              (.102, .012, .045), armor, hand, .007)
        c.box(f"{side} cuff patch", (wrist[0], wrist[1]-.043, wrist[2]),
              (.078, .008, .022), armor, hand, .003)

        for index in range(4):
            suffix = ["Index", "Middle", "Ring", "Pinky"][index]
            bone = f"{side}_{suffix}"
            x = palm[0] + (index-1.5)*.021
            base = (x, palm[1]-.002, palm[2]-.018)
            middle = (x, palm[1]+.007, finger_z+.024)
            tip = (x, palm[1]+.030, finger_z)
            c.segment(f"{side} {suffix} proximal", base, middle,
                      .011, .010, fabric, bone, 10)
            c.segment(f"{side} {suffix} tip", middle, tip,
                      .010, .008, rubber, bone, 10)
            c.ellipsoid(f"{side} {suffix} knuckle", base,
                        (.014, .016, .011), armor, bone, 10)
            c.box(f"{side} {suffix} stitch", (x, palm[1]-.017, palm[2]-.035),
                  (.004, .002, .020), seam, bone, .0007)
        c.segment(f"{side} thumb base", palm,
                  (thumb[0], thumb[1]-.006, thumb[2]+.015),
                  .017, .014, fabric, f"{side}_Thumb", 10)
        c.segment(f"{side} thumb tip", (thumb[0], thumb[1]-.006, thumb[2]+.015),
                  thumb, .014, .011, rubber, f"{side}_Thumb", 10)
        c.box(f"{side} palm seam", (palm[0], palm[1]-.041, palm[2]-.013),
              (.074, .002, .004), seam, hand, .001)
    c.rig(bones)
    c.export_subset("R", (-.120, -.125, -.135))
    c.export_subset("L", (.305, -.135, -.055))
    c.output((.62, -.95, .48), (.08, -.10, -.17), 1.80, .65)


def make_mannequin():
    c = Character("mannequin")
    shell = c.mat("01 Warm ceramic shell", (.57, .60, .60), .08, .62)
    undersuit = c.mat("02 Graphite joints", (.075, .085, .089), .08, .80)
    plates = c.mat("03 Mid-grey panels", (.32, .35, .36), .13, .65)
    accent = c.mat("04 Cyan markers", (.055, .35, .42), .20, .42)
    dark = c.mat("05 Face and soles", (.033, .041, .047), .12, .64)

    bones = [
        ("Root", (0, 0, 0), (0, 0, .92), None),
        ("Hips", (0, 0, .92), (0, 0, 1.02), "Root"),
        ("Spine", (0, 0, 1.02), (0, 0, 1.27), "Hips"),
        ("Chest", (0, 0, 1.27), (0, 0, 1.49), "Spine"),
        ("Neck", (0, 0, 1.49), (0, 0, 1.59), "Chest"),
        ("Head", (0, 0, 1.59), (0, 0, 1.79), "Neck"),
    ]
    for side, sign in (("L", -1), ("R", 1)):
        bones += [
            (f"{side}_UpperArm", (sign*.267, 0, 1.425),
             (sign*.450, 0, 1.185), "Chest"),
            (f"{side}_LowerArm", (sign*.450, 0, 1.185),
             (sign*.577, 0, .984), f"{side}_UpperArm"),
            (f"{side}_Hand", (sign*.577, 0, .984),
             (sign*.618, 0, .893), f"{side}_LowerArm"),
            (f"{side}_UpperLeg", (sign*.112, 0, .902),
             (sign*.135, 0, .526), "Hips"),
            (f"{side}_LowerLeg", (sign*.135, 0, .526),
             (sign*.141, 0, .130), f"{side}_UpperLeg"),
            (f"{side}_Foot", (sign*.141, 0, .130),
             (sign*.141, .108, .055), f"{side}_LowerLeg"),
        ]

    # Torso and head are simple, faceless forms suitable for the starter demo.
    c.ellipsoid("Pelvis shell", (0, 0, .965), (.205, .108, .130), shell, "Hips")
    c.box("Pelvis center panel", (0, .102, .958), (.097, .019, .085), plates, "Hips", .012)
    c.segment("Waist", (0, 0, 1.018), (0, 0, 1.165), .142, .160,
              undersuit, "Spine")
    c.ellipsoid("Upper torso", (0, 0, 1.326), (.288, .134, .237), shell, "Chest")
    c.box("Chest front panel", (0, .121, 1.342), (.336, .030, .205),
          plates, "Chest", .024)
    c.box("Chest ID marker", (0, .140, 1.398), (.063, .008, .018),
          accent, "Chest", .004)
    for sign in (-1, 1):
        c.box("Clavicle seam", (sign*.172, .128, 1.454),
              (.094, .008, .011), undersuit, "Chest", .003)
    c.segment("Neck seal", (0, 0, 1.490), (0, 0, 1.592),
              .060, .055, undersuit, "Neck")
    c.ellipsoid("Head", (0, 0, 1.692), (.118, .104, .151), shell, "Head")
    c.box("Faceless visor", (0, .094, 1.706), (.159, .017, .095),
          dark, "Head", .023)
    c.box("Visor signal line", (0, .105, 1.676), (.099, .005, .008),
          accent, "Head", .002)
    c.ellipsoid("Back head panel", (0, -.082, 1.690),
                (.094, .025, .104), plates, "Head")

    # A-pose limbs, with visible joint gaps and individual upper/lower bones.
    for side, sign in (("L", -1), ("R", 1)):
        upper_arm = f"{side}_UpperArm"
        lower_arm = f"{side}_LowerArm"
        hand = f"{side}_Hand"
        upper_leg = f"{side}_UpperLeg"
        lower_leg = f"{side}_LowerLeg"
        foot = f"{side}_Foot"
        shoulder = (sign*.280, 0, 1.410)
        elbow = (sign*.450, 0, 1.185)
        wrist = (sign*.577, 0, .984)
        c.ellipsoid(f"{side} shoulder joint", shoulder,
                    (.071, .073, .071), undersuit, upper_arm)
        c.segment(f"{side} upper arm", shoulder, elbow,
                  .084, .063, shell, upper_arm)
        c.ellipsoid(f"{side} elbow joint", elbow,
                    (.060, .058, .060), undersuit, lower_arm)
        c.segment(f"{side} forearm", elbow, wrist,
                  .065, .046, plates, lower_arm)
        c.ellipsoid(f"{side} wrist joint", wrist,
                    (.046, .043, .046), undersuit, hand)
        c.ellipsoid(f"{side} hand", (sign*.612, 0, .914),
                    (.051, .038, .081), shell, hand)
        c.box(f"{side} hand marker", (sign*.612, .035, .944),
              (.033, .007, .014), accent, hand, .002)

        hip = (sign*.112, 0, .902)
        knee = (sign*.135, 0, .526)
        ankle = (sign*.141, 0, .130)
        c.ellipsoid(f"{side} hip joint", hip,
                    (.103, .082, .095), undersuit, upper_leg)
        c.segment(f"{side} thigh", hip, knee,
                  .105, .075, shell, upper_leg)
        c.box(f"{side} thigh front panel", (sign*.124, .078, .729),
              (.110, .018, .197), plates, upper_leg, .017)
        c.ellipsoid(f"{side} knee joint", knee,
                    (.076, .072, .079), undersuit, lower_leg)
        c.box(f"{side} kneecap", (sign*.135, .065, .537),
              (.106, .024, .086), shell, lower_leg, .016)
        c.segment(f"{side} shin", knee, ankle,
                  .080, .052, shell, lower_leg)
        c.box(f"{side} shin insert", (sign*.142, .062, .320),
              (.080, .017, .205), plates, lower_leg, .014)
        c.ellipsoid(f"{side} ankle joint", ankle,
                    (.053, .053, .057), undersuit, foot)
        c.ellipsoid(f"{side} foot", (sign*.141, .076, .069),
                    (.079, .154, .068), shell, foot)
        c.box(f"{side} sole", (sign*.141, .083, .022),
              (.162, .274, .026), dark, foot, .010)
        c.box(f"{side} toe marker", (sign*.141, .186, .077),
              (.092, .010, .011), accent, foot, .002)
    c.rig(bones)
    c.output((2.65, 4.15, 2.32), (0, 0, .91), 4.00, .72)


make_hands()
if "--hands-only" not in sys.argv:
    make_mannequin()
