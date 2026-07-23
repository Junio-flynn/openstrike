"""Convert the user-supplied bolt-action sniper archive into OpenStrike GLB.

Run with:
    blender --background --factory-startup --python scripts/build-sniper-viewmodel.py

Blender 5's headless FBX operator is incomplete on this environment, so the
script calls the bundled loader directly, matching build-mixamo-swat.py.
"""

from pathlib import Path
from tempfile import TemporaryDirectory
from zipfile import ZipFile

import bpy
from io_scene_fbx import import_fbx


PROJECT = Path(__file__).resolve().parent.parent
ARCHIVE = Path.home() / "Downloads" / "fps-animations-sniper-rifle.zip"
OUTPUT = PROJECT / "assets" / "models" / "BoltActionSniper_Viewmodel.glb"
FBX_MEMBER = "source/BoltActionRifle.fbx"
CLIPS = {
    "SRifle_Idle": "Idle",
    "SRifle_Reload": "Reload",
    "SRifle_Reload_Full": "Reload_Empty",
    "SRifle_Shot_nosight": "Fire",
    "SRifle_Shot_sight": "Fire_Aimed",
    "SRifle_Walk": "Walk",
}


class ImportReporter:
    def report(self, levels, message):
        print(f"FBX {','.join(levels)}: {message}")


def main():
    if not ARCHIVE.is_file():
        raise FileNotFoundError(ARCHIVE)
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    # Blender 5 removed a Cycles light property while its bundled FBX importer
    # still assigns it. The light is irrelevant to a viewmodel, so import a
    # harmless placeholder and discard non-model scene objects below.
    import_fbx.blen_read_light = lambda _template, _object, _settings: bpy.data.lights.new(
        name="FBX Light", type="POINT"
    )

    with TemporaryDirectory(prefix="openstrike-sniper-") as temporary:
        root = Path(temporary)
        with ZipFile(ARCHIVE) as archive:
            members = archive.namelist()
            if FBX_MEMBER not in members:
                raise RuntimeError(f"Archive is missing {FBX_MEMBER}")
            for member in members:
                path = Path(member)
                if path.is_absolute() or ".." in path.parts:
                    raise RuntimeError(f"Unsafe archive path: {member}")
            archive.extractall(root)

        result = import_fbx.load(
            ImportReporter(),
            bpy.context,
            filepath=str(root / FBX_MEMBER),
            use_image_search=True,
            use_anim=True,
        )
        if result != {"FINISHED"}:
            raise RuntimeError(f"Could not import sniper FBX: {result}")

        for obj in list(bpy.context.scene.objects):
            if obj.type in {"LIGHT", "CAMERA"}:
                bpy.data.objects.remove(obj, do_unlink=True)

        print("Imported objects:")
        for obj in bpy.context.scene.objects:
            detail = len(obj.data.vertices) if obj.type == "MESH" else ""
            print(f"  {obj.type:8} {obj.name} {detail}")
        print("Imported actions:")
        for action in bpy.data.actions:
            print(f"  {action.name}: frames {tuple(round(v) for v in action.frame_range)}")

        # FBX generates duplicate actions for its removed camera and light.
        # Keep only the actions authored against the real Rig and give them
        # the clip names consumed by OpenStrike's viewmodel selector.
        kept_actions = []
        for action in list(bpy.data.actions):
            source_name = next((name for name in CLIPS if action.name.endswith(name)), None)
            if action.name.startswith("Rig|Rig|") and source_name is not None:
                action.name = CLIPS[source_name]
                kept_actions.append(action)
            else:
                bpy.data.actions.remove(action)

        # The FBX references only the arms' roughness map, so Blender's FBX
        # importer otherwise exports white gloves and sleeves. Rebuild that
        # material explicitly from the supplied color, normal, and roughness
        # textures before the shared resize/pack pass below.
        arms_material = bpy.data.materials.get("arms")
        if arms_material is not None:
            arms_material.use_nodes = True
            nodes = arms_material.node_tree.nodes
            nodes.clear()
            output = nodes.new("ShaderNodeOutputMaterial")
            shader = nodes.new("ShaderNodeBsdfPrincipled")
            arms_material.node_tree.links.new(shader.outputs["BSDF"], output.inputs["Surface"])

            color = bpy.data.images.load(str(root / "textures" / "armColor.png"), check_existing=True)
            color.colorspace_settings.name = "sRGB"
            color_node = nodes.new("ShaderNodeTexImage")
            color_node.image = color
            arms_material.node_tree.links.new(color_node.outputs["Color"], shader.inputs["Base Color"])

            roughness = bpy.data.images.load(
                str(root / "textures" / "armRoughness.png"), check_existing=True
            )
            roughness.colorspace_settings.name = "Non-Color"
            roughness_node = nodes.new("ShaderNodeTexImage")
            roughness_node.image = roughness
            arms_material.node_tree.links.new(
                roughness_node.outputs["Color"], shader.inputs["Roughness"]
            )

            normal = bpy.data.images.load(str(root / "textures" / "armNormal.png"), check_existing=True)
            normal.colorspace_settings.name = "Non-Color"
            normal_texture = nodes.new("ShaderNodeTexImage")
            normal_texture.image = normal
            normal_node = nodes.new("ShaderNodeNormalMap")
            arms_material.node_tree.links.new(normal_texture.outputs["Color"], normal_node.inputs["Color"])
            arms_material.node_tree.links.new(normal_node.outputs["Normal"], shader.inputs["Normal"])

        # The source carries very large maps. Two-kilopixel sources retain
        # first-person detail while keeping the embedded GLB practical.
        for image in bpy.data.images:
            candidate = root / "textures" / image.name
            if candidate.is_file():
                image.filepath = str(candidate)
                image.reload()
            width, height = image.size
            longest = max(width, height)
            if longest > 2048:
                factor = 2048.0 / longest
                image.scale(max(1, round(width * factor)), max(1, round(height * factor)))
            if image.has_data:
                image.pack()

        # The arms material carries an integer UV offset, which is visually
        # identical under repeat wrapping but makes Blender require
        # KHR_texture_transform. Pocket3D intentionally accepts core glTF
        # only, so normalize such no-op mapping offsets before export.
        for material in bpy.data.materials:
            if material.node_tree is None:
                continue
            for node in material.node_tree.nodes:
                if node.type == "MAPPING":
                    location = node.inputs.get("Location")
                    if location is not None:
                        location.default_value = (0.0, 0.0, 0.0)

        armatures = [obj for obj in bpy.context.scene.objects if obj.type == "ARMATURE"]
        if armatures and kept_actions:
            armature = armatures[0]
            if armature.animation_data is None:
                armature.animation_data_create()
            armature.animation_data.action = kept_actions[0]

        bpy.ops.object.select_all(action="SELECT")
        OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        bpy.ops.export_scene.gltf(
            filepath=str(OUTPUT),
            export_format="GLB",
            use_selection=True,
            export_animations=True,
            export_animation_mode="ACTIONS",
            export_merge_animation="ACTION",
            export_anim_single_armature=True,
            export_optimize_animation_size=True,
            export_skins=True,
            export_materials="EXPORT",
            export_image_format="AUTO",
        )
    print(f"Wrote {OUTPUT}")


if __name__ == "__main__":
    main()
