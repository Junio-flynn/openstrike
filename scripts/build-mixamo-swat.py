"""Build OpenStrike's third-person SWAT GLB from user-provided Mixamo FBX files.

Run with:
    blender --background --factory-startup --python scripts/build-mixamo-swat.py

Blender 5's FBX operator currently omits the ImportHelper properties in some
headless builds, so this script calls the bundled FBX loader directly.
"""

from pathlib import Path

import bpy
from io_scene_fbx import import_fbx


PROJECT = Path(__file__).resolve().parent.parent
DOWNLOADS = Path.home() / "Downloads"
OUTPUT = PROJECT / "assets" / "models" / "Soldier.glb"

# Export names are part of the runtime contract in game.rs.
CLIPS = (
    ("Walking.fbx", "Walk"),
    ("Rifle Run.fbx", "Run"),
    ("Crouch Walking.fbx", "CrouchWalk"),
    ("Crouch Walking Backwards.fbx", "CrouchWalkBack"),
    ("Running Slide.fbx", "Slide"),
    ("Rifle Death.fbx", "Death"),
    ("Walking To Dying.fbx", "WalkDeath"),
    ("Walking Hit Reaction.fbx", "Hit"),
    ("Fire Rifle.fbx", "Fire"),
    ("Shoot Rifle.fbx", "Shoot"),
    ("Firing Rifle.fbx", "Firing"),
    ("Firing Rifle(1).fbx", "FiringAlt"),
    ("Rifle Stand To Kneel.fbx", "CrouchEnter"),
    ("Crouch To Standing With Rifle.fbx", "CrouchExit"),
    ("Walking Backwards.fbx", "WalkBack"),
    ("Start Walking.fbx", "WalkStart"),
    ("Stop Walking.fbx", "WalkStop"),
    ("Start Walking Backwards.fbx", "WalkBackStart"),
    ("Crouch Stop Walking.fbx", "CrouchStop"),
    ("Jump Up.fbx", "JumpUp"),
    ("Jump Loop.fbx", "JumpLoop"),
    ("Jump Down.fbx", "JumpDown"),
    ("Jump Forward.fbx", "JumpForward"),
)

IN_PLACE_CLIPS = {
    "Walk",
    "Run",
    "CrouchWalk",
    "CrouchWalkBack",
    "Slide",
    "WalkBack",
    "WalkBackStart",
    "WalkStart",
    "WalkStop",
    "CrouchStop",
    "CrouchEnter",
    "CrouchExit",
    "JumpUp",
    "JumpLoop",
    "JumpDown",
    "JumpForward",
}


class ImportReporter:
    def report(self, levels, message):
        print(f"FBX {','.join(levels)}: {message}")


def import_fbx_file(path):
    before_objects = set(bpy.data.objects)
    before_actions = set(bpy.data.actions)
    result = import_fbx.load(
        ImportReporter(),
        bpy.context,
        filepath=str(path),
        use_image_search=True,
        use_anim=True,
    )
    if result != {"FINISHED"}:
        raise RuntimeError(f"Could not import {path}: {result}")
    objects = [obj for obj in bpy.data.objects if obj not in before_objects]
    actions = [action for action in bpy.data.actions if action not in before_actions]
    if len(actions) != 1:
        raise RuntimeError(f"Expected one action in {path.name}, found {len(actions)}")
    return objects, actions[0]


def make_static_pose(source, name, frame=None):
    """Freeze one action pose into a two-frame action."""
    idle = source.copy()
    idle.name = name
    sample_frame = source.frame_range[1] if frame is None else frame
    for layer in idle.layers:
        for strip in layer.strips:
            for slot in idle.slots:
                channelbag = strip.channelbag(slot, ensure=False)
                if channelbag is None:
                    continue
                for curve in channelbag.fcurves:
                    value = curve.evaluate(sample_frame)
                    curve.keyframe_points.clear()
                    first = curve.keyframe_points.insert(1.0, value)
                    last = curve.keyframe_points.insert(2.0, value)
                    first.interpolation = "CONSTANT"
                    last.interpolation = "CONSTANT"
    idle.use_fake_user = True
    return idle


def lowest_bone_frame(action, armature, bone_name):
    """Find the deepest pose in an action in Blender world-up space."""
    if armature.animation_data is None:
        armature.animation_data_create()
    armature.animation_data.action = action
    bone = armature.pose.bones.get(bone_name)
    if bone is None:
        return sum(action.frame_range) * 0.5
    best_frame = action.frame_range[0]
    best_height = float("inf")
    start, end = (int(action.frame_range[0]), int(action.frame_range[1]))
    for frame in range(start, end + 1):
        bpy.context.scene.frame_set(frame)
        height = (armature.matrix_world @ bone.matrix).translation.z
        if height < best_height:
            best_height = height
            best_frame = frame
    return best_frame


def remove_root_travel(action):
    """Make Mixamo locomotion in-place; OpenStrike physics owns translation."""
    start_frame, end_frame = action.frame_range
    duration = max(end_frame - start_frame, 1.0)
    for layer in action.layers:
        for strip in layer.strips:
            for slot in action.slots:
                channelbag = strip.channelbag(slot, ensure=False)
                if channelbag is None:
                    continue
                for curve in channelbag.fcurves:
                    if not curve.data_path.endswith('mixamorig:Hips"].location'):
                        continue
                    # Mixamo's bone-local axes are rotated again during glTF
                    # export, so no individual Blender component reliably
                    # corresponds to world-up. Physics owns all player/root
                    # translation; retain the pose but detrend every hips axis.
                    if curve.array_index not in (0, 1, 2):
                        continue
                    travel = curve.evaluate(end_frame) - curve.evaluate(start_frame)
                    if abs(travel) < 0.01:
                        continue
                    for key in curve.keyframe_points:
                        fraction = (key.co.x - start_frame) / duration
                        offset = travel * fraction
                        key.co.y -= offset
                        key.handle_left.y -= offset
                        key.handle_right.y -= offset


def main():
    missing = [DOWNLOADS / filename for filename, _ in CLIPS if not (DOWNLOADS / filename).is_file()]
    if missing:
        raise FileNotFoundError("Missing Mixamo files: " + ", ".join(str(path) for path in missing))

    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for action in list(bpy.data.actions):
        bpy.data.actions.remove(action)

    base_objects = None
    base_armature = None
    imported_actions = {}

    for index, (filename, clip_name) in enumerate(CLIPS):
        objects, action = import_fbx_file(DOWNLOADS / filename)
        action.name = clip_name
        action.use_fake_user = True
        if clip_name in IN_PLACE_CLIPS:
            remove_root_travel(action)
        imported_actions[clip_name] = action

        armatures = [obj for obj in objects if obj.type == "ARMATURE"]
        meshes = [obj for obj in objects if obj.type == "MESH"]
        if len(armatures) != 1 or len(meshes) < 1:
            raise RuntimeError(
                f"Unexpected contents in {filename}: {len(armatures)} armatures, {len(meshes)} meshes"
            )

        if index == 0:
            base_objects = armatures + meshes
            base_armature = armatures[0]
            base_armature.name = "SWAT_Armature"
            for number, mesh in enumerate(meshes, start=1):
                mesh.name = f"SWAT_Mesh_{number}"
        else:
            for obj in objects:
                bpy.data.objects.remove(obj, do_unlink=True)

    idle = make_static_pose(imported_actions["WalkStop"], "Idle")
    make_static_pose(imported_actions["CrouchEnter"], "CrouchIdle")
    slide_frame = lowest_bone_frame(imported_actions["Slide"], base_armature, "mixamorig:Head")
    make_static_pose(imported_actions["Slide"], "SlideLoop", slide_frame)

    # Assign one action so the armature has a valid action slot. ACTIONS mode
    # exports every compatible action datablock as a separate glTF clip.
    if base_armature.animation_data is None:
        base_armature.animation_data_create()
    base_armature.animation_data.action = idle

    bpy.ops.object.select_all(action="DESELECT")
    for obj in base_objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = base_armature

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
    print("Clips:", ", ".join(sorted(action.name for action in bpy.data.actions)))


if __name__ == "__main__":
    main()
