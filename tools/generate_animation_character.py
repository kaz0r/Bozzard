"""Build original stylized humans and phase-matched animation clips in Blender.

Run: blender --background --factory-startup --python tools/generate_animation_character.py
No downloaded meshes, textures, rigs or animations are used. Geometry is in metres.
"""
from pathlib import Path
import math
import sys

import bpy
from mathutils import Quaternion, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from blender_helpers import create_principled_material

ROOT = Path(__file__).resolve().parents[1] / "assets" / "animation-human"
FPS = 30


class Human:
    def __init__(self, name, height=1.0, width=1.0, color=(.06, .32, .43)):
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete(use_global=False)
        self.name, self.height, self.width = name, height, width
        self.parts, self.bindings = [], {}
        self.materials = {
            "skin": create_principled_material(bpy, "Warm skin", (.58, .30, .18), roughness=.72),
            "shirt": create_principled_material(bpy, "Cotton jacket", color, roughness=.83),
            "pants": create_principled_material(bpy, "Charcoal trousers", (.065, .083, .11), roughness=.86),
            "shoes": create_principled_material(bpy, "Sneakers", (.17, .19, .22), roughness=.7),
            "white": create_principled_material(bpy, "Cream details", (.82, .85, .79), roughness=.62),
            "dark": create_principled_material(bpy, "Hair and pupils", (.033, .018, .016), roughness=.72),
        }

    def point(self, point):
        return Vector((point[0] * self.width, point[1], point[2] * self.height))

    def part(self, obj, name, material, bone):
        obj.name = name
        obj.data.materials.append(self.materials[material])
        for polygon in obj.data.polygons:
            polygon.use_smooth = True
        self.parts.append(obj)
        self.bindings[obj] = bone
        return obj

    def sphere(self, name, position, radius, material, bone, segments=16):
        bpy.ops.mesh.primitive_uv_sphere_add(segments=segments, ring_count=10, location=self.point(position))
        obj = bpy.context.object
        obj.scale = (radius[0] * self.width, radius[1], radius[2] * self.height)
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        return self.part(obj, name, material, bone)

    def limb(self, name, start, end, radius, material, bone):
        a, b = self.point(start), self.point(end)
        obj = self.sphere(name, (0, 0, 0), (radius, radius, (b-a).length / (2*self.height) + radius*.25), material, bone)
        obj.location = (a+b)/2
        obj.rotation_euler = (b-a).to_track_quat("Z", "Y").to_euler()
        return obj

    def torso(self):
        # A continuous cloth mesh, smoothly weighted across the hips, spine and chest.
        rings = [(1.0, .15, .115), (1.09, .17, .12), (1.22, .16, .13),
                 (1.35, .22, .135), (1.45, .245, .125), (1.49, .20, .105), (1.54, .075, .068)]
        vertices = [self.point((rx*math.cos(i*math.tau/20), ry*math.sin(i*math.tau/20), z))
                    for z, rx, ry in rings for i in range(20)]
        faces = [tuple(reversed(range(20)))]
        faces += [(j*20+i, j*20+(i+1)%20, (j+1)*20+(i+1)%20, (j+1)*20+i)
                  for j in range(len(rings)-1) for i in range(20)]
        faces.append(tuple(range((len(rings)-1)*20, len(rings)*20)))
        mesh = bpy.data.meshes.new("Jacket cloth")
        mesh.from_pydata(vertices, [], faces)
        mesh.update()
        obj = bpy.data.objects.new("Jacket", mesh)
        bpy.context.collection.objects.link(obj)
        self.part(obj, "Jacket", "shirt", "Spine")

    def build(self):
        self.torso()
        self.sphere("Pelvis", (0, 0, 1.02), (.18, .125, .135), "pants", "Hips")
        self.sphere("Neck", (0, 0, 1.56), (.065, .061, .095), "skin", "Neck")
        self.sphere("Head", (0, .012, 1.715), (.112, .10, .145), "skin", "Head", 24)
        self.sphere("Jaw", (0, .045, 1.64), (.092, .085, .059), "skin", "Head")
        self.sphere("Hair cap", (0, -.012, 1.80), (.115, .098, .09), "dark", "Head")
        self.sphere("Hair fringe", (-.045, .087, 1.785), (.070, .025, .035), "dark", "Head")
        self.sphere("Nose", (0, .108, 1.70), (.024, .035, .033), "skin", "Head")
        self.sphere("Mouth", (0, .116, 1.658), (.034, .007, .006), "dark", "Head")
        self.limb("Jacket zipper", (0, .137, 1.09), (0, .137, 1.48), .009, "white", "Spine")
        self.sphere("Collar", (0, -.005, 1.54), (.079, .07, .03), "shirt", "Chest")
        bones = [("Root", (0, 0, 0), (0, 0, .18), None),
                 ("Hips", (0, 0, 1.02), (0, 0, 1.16), "Root"),
                 ("Spine", (0, 0, 1.16), (0, 0, 1.34), "Hips"),
                 ("Chest", (0, 0, 1.34), (0, 0, 1.49), "Spine"),
                 ("Neck", (0, 0, 1.49), (0, 0, 1.62), "Chest"),
                 ("Head", (0, 0, 1.62), (0, 0, 1.86), "Neck")]
        for side, sign in [("Left", -1), ("Right", 1)]:
            self.face(side, sign)
            shoulder, elbow, wrist = (sign*.23, 0, 1.45), (sign*.40, .015, 1.15), (sign*.51, .035, .90)
            hip, knee, ankle = (sign*.105, 0, 1.02), (sign*.115, .025, .57), (sign*.12, 0, .12)
            for suffix, head, tail, parent in [
                ("UpperArm", shoulder, elbow, "Chest"), ("LowerArm", elbow, wrist, side+"UpperArm"),
                ("Hand", wrist, (sign*.54, .05, .79), side+"LowerArm"),
                ("UpperLeg", hip, knee, "Hips"), ("LowerLeg", knee, ankle, side+"UpperLeg"),
                ("Foot", ankle, (sign*.12, .20, .08), side+"LowerLeg")]:
                bones.append((side+suffix, head, tail, parent))
            self.limb(side+" sleeve", shoulder, elbow, .080, "shirt", side+"UpperArm")
            self.limb(side+" forearm", elbow, wrist, .058, "shirt", side+"LowerArm")
            self.sphere(side+" elbow", elbow, (.063, .063, .060), "shirt", side+"LowerArm")
            self.sphere(side+" cuff", wrist, (.050, .050, .030), "white", side+"LowerArm")
            self.sphere(side+" palm", (sign*.527, .04, .852), (.044, .028, .065), "skin", side+"Hand")
            for finger in range(4):
                x = sign*(.499+finger*.017)
                self.limb(side+f" finger {finger}", (x, .05, .827), (x+sign*.008, .063, .777+abs(finger-1)*.008),
                          .010, "skin", side+"Hand")
            self.limb(side+" thumb", (sign*.49, .06, .87), (sign*.471, .085, .835), .015, "skin", side+"Hand")
            self.limb(side+" thigh", hip, knee, .094, "pants", side+"UpperLeg")
            self.limb(side+" shin", knee, ankle, .067, "pants", side+"LowerLeg")
            self.sphere(side+" knee", knee, (.073, .073, .075), "pants", side+"LowerLeg")
            self.sphere(side+" shoe", (sign*.12, .083, .077), (.085, .157, .072), "shoes", side+"Foot")
            self.sphere(side+" sole", (sign*.12, .085, .030), (.087, .158, .027), "white", side+"Foot")
            self.sphere(side+" laces", (sign*.12, .145, .119), (.048, .043, .010), "white", side+"Foot")
        self.rig(bones)

    def face(self, side, sign):
        self.sphere(side+" ear", (sign*.112, .005, 1.707), (.026, .025, .041), "skin", "Head")
        self.sphere(side+" eye", (sign*.042, .101, 1.734), (.024, .012, .013), "white", "Head")
        self.sphere(side+" iris", (sign*.042, .112, 1.734), (.008, .003, .009), "dark", "Head")
        self.limb(side+" eyebrow", (sign*.025, .104, 1.755), (sign*.065, .100, 1.758), .007, "dark", "Head")

    def rig(self, bones):
        data = bpy.data.armatures.new("Human skeleton")
        self.armature = bpy.data.objects.new("HumanRig", data)
        bpy.context.collection.objects.link(self.armature)
        bpy.context.view_layer.objects.active = self.armature
        self.armature.select_set(True)
        bpy.ops.object.mode_set(mode="EDIT")
        for name, head, tail, parent in bones:
            bone = data.edit_bones.new(name)
            bone.head, bone.tail = self.point(head), self.point(tail)
            if parent:
                bone.parent = data.edit_bones[parent]
        bpy.ops.object.mode_set(mode="OBJECT")
        for obj in self.parts:
            groups = {name: obj.vertex_groups.new(name=name) for name in ("Hips", "Spine", "Chest")}
            binding = self.bindings[obj]
            if binding not in groups:
                groups[binding] = obj.vertex_groups.new(name=binding)
            for vertex in obj.data.vertices:
                if obj.name == "Jacket":
                    z = vertex.co.z/self.height
                    t = min(2., max(0., (z-1.02)/.24))
                    low = min(1, int(t))
                    for i, weight in [(low, 1-(t-low)), (low+1, t-low)]:
                        if weight > 0:
                            groups[("Hips", "Spine", "Chest")[i]].add([vertex.index], weight, "REPLACE")
                else:
                    groups[binding].add([vertex.index], 1., "REPLACE")
            obj.parent = self.armature
            modifier = obj.modifiers.new("Human skin", "ARMATURE")
            modifier.object = self.armature
        self.armature.show_in_front = True

    def pose(self, rotations, hips=0., root=0.):
        for bone in self.armature.pose.bones:
            bone.rotation_mode = "QUATERNION"
            basis = bone.bone.matrix_local.to_quaternion()
            bone.rotation_quaternion = basis.inverted() @ rotations.get(bone.name, Quaternion()) @ basis
            bone.location = (0, 0, 0)
        # Bone-space translations are converted from Blender's world axes.
        for name, delta in [("Hips", (0, 0, hips*self.height)), ("Root", (0, root, 0))]:
            bone = self.armature.pose.bones[name]
            bone.location = bone.bone.matrix_local.to_quaternion().inverted() @ Vector(delta)

    def animate(self, name, seconds, sampler):
        action = bpy.data.actions.new(name)
        action.use_fake_user = True
        self.armature.animation_data_create()
        self.armature.animation_data.action = action
        count = round(seconds*FPS)
        for frame in range(count+1):
            self.pose(*sampler(frame/count))
            for bone in self.armature.pose.bones:
                bone.keyframe_insert("rotation_quaternion", frame=frame)
                if bone.name in ("Root", "Hips"):
                    bone.keyframe_insert("location", frame=frame)
        return action

    def export(self, animated=True):
        self.pose({})
        self.armature.animation_data_create()
        self.armature.animation_data.action = None
        bpy.context.scene.render.fps = FPS
        bpy.ops.object.select_all(action="DESELECT")
        self.armature.select_set(True)
        for part in self.parts:
            part.select_set(True)
        bpy.context.view_layer.objects.active = self.armature
        bpy.ops.wm.save_as_mainfile(filepath=str(ROOT / (self.name+".blend")))
        # Keep the editable parts in .blend, but publish one skin and six material primitives.
        # Separate objects would duplicate all 18 joint bindings for every part at runtime.
        self.armature.select_set(False)
        bpy.context.view_layer.objects.active = self.parts[0]
        bpy.ops.object.join()
        self.armature.select_set(True)
        bpy.ops.export_scene.gltf(filepath=str(ROOT / (self.name+".glb")), export_format="GLB",
                                  use_selection=True, export_yup=True, export_skins=True,
                                  export_animations=animated, export_animation_mode="ACTIONS",
                                  export_frame_range=False, export_force_sampling=True)


def rotation(axis, angle):
    return Quaternion(axis, angle)


def gait(phase, stride=.40, direction=0., running=False):
    rotations = {"Spine": rotation((1, 0, 0), -.08 if running else -.025),
                 "Chest": rotation((0, 0, 1), .07*math.sin(phase*math.tau))}
    for side, sign in [("Left", 1), ("Right", -1)]:
        cycle = phase*math.tau + (0 if sign == 1 else math.pi)
        swing = math.sin(cycle)
        thigh = stride*swing
        knee = -.14 - max(0., swing)*(.95 if running else .55)
        tilt = rotation((1, 0, 0), thigh*math.cos(direction)) @ rotation((0, 1, 0), thigh*math.sin(direction))
        rotations[side+"UpperLeg"] = tilt
        rotations[side+"LowerLeg"] = rotation((1, 0, 0), knee)
        rotations[side+"Foot"] = rotation((1, 0, 0), -thigh*math.cos(direction)-knee)
        arm = -.50*swing if running else -.23*swing
        rotations[side+"UpperArm"] = rotation((0, 1, 0), -.35*sign) @ rotation((1, 0, 0), arm)
        rotations[side+"LowerArm"] = rotation((1, 0, 0), .75 if running else .30)
    return rotations, -.015 + (.028 if running else .016)*math.cos(phase*math.tau*2), 0.


def idle(phase):
    return {"Chest": rotation((1, 0, 0), .018*math.sin(phase*math.tau)),
            "LeftUpperArm": rotation((0, 1, 0), -.35),
            "RightUpperArm": rotation((0, 1, 0), .35)}, .004*math.sin(phase*math.tau), 0.


def gesture(phase, aim=False):
    pose, hips, root = idle(phase)
    pose["RightUpperArm"] = rotation((1, 0, 0), 1.22) @ rotation((0, 1, 0), .32)
    pose["RightLowerArm"] = rotation((1, 0, 0), .32 if aim else 1.20)
    pose["RightHand"] = rotation((0, 1, 0), .08 if aim else .42*math.sin(phase*math.tau*2))
    if aim:
        pose["LeftUpperArm"] = rotation((1, 0, 0), 1.10) @ rotation((0, 1, 0), -.52)
        pose["LeftLowerArm"] = rotation((1, 0, 0), .70)
    return pose, hips, root


def jump(phase):
    lift = math.sin(math.pi*phase)
    pose, _, _ = gait(phase, .0)
    for side in ["Left", "Right"]:
        pose[side+"UpperLeg"] = rotation((1, 0, 0), .30*lift)
        pose[side+"LowerLeg"] = rotation((1, 0, 0), -.70*lift)
        pose[side+"Foot"] = rotation((1, 0, 0), .40*lift)
        pose[side+"UpperArm"] = rotation((1, 0, 0), -.50*lift)
    return pose, .55*lift, 0.


def reach(phase):
    pose, hips, _ = gait(phase*1.5, .32)
    pose["RightUpperArm"] = rotation((1, 0, 0), 1.25*min(1., phase*3))
    pose["RightLowerArm"] = rotation((1, 0, 0), .25)
    return pose, hips, 1.15*phase


def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    bpy.context.preferences.filepaths.save_version = 0
    human = Human("human")
    human.build()
    human.animate("Idle", 2., idle)
    for name, direction in [("WalkForward", 0.), ("WalkBack", math.pi),
                            ("WalkLeft", math.pi/2), ("WalkRight", -math.pi/2)]:
        human.animate(name, 1., lambda p, d=direction: gait(p, direction=d))
    human.animate("RunForward", .7, lambda p: gait(p, .73, running=True))
    human.animate("Wave", 1.6, gesture)
    human.animate("Aim", 2., lambda p: gesture(p, True))
    human.animate("Jump", .9, jump)
    human.animate("Reach", 1.5, reach)
    human.export()
    # A different bind pose/proportion with no clips: the engine supplies retargeted motion.
    for action in list(bpy.data.actions):
        bpy.data.actions.remove(action)
    tall = Human("human-tall", height=1.16, width=.91, color=(.49, .17, .07))
    tall.build()
    tall.export(False)
    print("animation_humans_exported", ROOT)


if __name__ == "__main__":
    main()
