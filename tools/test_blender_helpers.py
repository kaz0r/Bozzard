"""Blender-free checks for shared asset construction helpers."""

import unittest
from types import SimpleNamespace
from unittest.mock import Mock

from blender_helpers import create_extruded_profile, create_principled_material


def fake_bpy():
    bpy = Mock()
    material = SimpleNamespace(node_tree=SimpleNamespace(nodes=Mock()))
    shader = SimpleNamespace(inputs={
        "Base Color": SimpleNamespace(default_value=None),
        "Metallic": SimpleNamespace(default_value=None),
        "Roughness": SimpleNamespace(default_value=None),
    })
    material.node_tree.nodes.get.return_value = shader
    bpy.data.materials.new.return_value = material

    mesh = Mock()
    bpy.data.meshes.new.return_value = mesh
    bpy.data.objects.new.side_effect = lambda name, data: SimpleNamespace(name=name, data=data)
    return bpy, material, shader, mesh


class BlenderHelpersTests(unittest.TestCase):
    def test_material_sets_viewport_and_shader_values(self):
        bpy, material, shader, _ = fake_bpy()
        color = (.1, .2, .3)

        result = create_principled_material(bpy, "Fabric", color, .4, .7)

        self.assertIs(result, material)
        self.assertEqual(material.diffuse_color, (*color, 1))
        self.assertTrue(material.use_nodes)
        self.assertEqual(shader.inputs["Base Color"].default_value, (*color, 1))
        self.assertEqual(shader.inputs["Metallic"].default_value, .4)
        self.assertEqual(shader.inputs["Roughness"].default_value, .7)

    def test_profile_keeps_extrusion_topology_and_center(self):
        bpy, _, _, mesh = fake_bpy()
        bpy.context.collection.objects.link = Mock()

        result = create_extruded_profile(
            bpy, "Panel", [(0, 0), (2, 0), (1, 1)], 0.5, 0.5
        )

        self.assertEqual(result.name, "Panel")
        mesh.from_pydata.assert_called_once_with(
            [(0, .25, 0), (2, .25, 0), (1, .25, 1),
             (0, .75, 0), (2, .75, 0), (1, .75, 1)],
            [],
            [(0, 1, 2), (5, 4, 3), (3, 4, 1, 0),
             (4, 5, 2, 1), (5, 3, 0, 2)],
        )
        mesh.update.assert_called_once_with()
        bpy.context.collection.objects.link.assert_called_once_with(result)


if __name__ == "__main__":
    unittest.main()
