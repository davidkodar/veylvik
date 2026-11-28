import bpy, math, sys
bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.frame_start, scene.frame_end = 1, 25
scene.render.fps = 24
# A 2 x 2 card at the origin, facing the camera (+Y in Blender's Z-up world faces -Y).
bpy.ops.mesh.primitive_plane_add(size=2, location=(0, 0, 0), rotation=(math.pi / 2, 0, 0))
card = bpy.context.object; card.name = "Card"
m = bpy.data.materials.new("Red"); m.use_nodes = True
m.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (1, 0, 0, 1)
card.data.materials.append(m)
# A cube that travels along x.
bpy.ops.mesh.primitive_cube_add(size=0.5, location=(-2, 1, 0))
cube = bpy.context.object; cube.name = "Mover"
cube.animation_data_create(); cube.animation_data.action = bpy.data.actions.new("MoverAction")
cube.keyframe_insert("location", frame=1); cube.location = (2, 1, 0); cube.keyframe_insert("location", frame=25)
# A camera 5 in front (at -Y), looking at the card, 50 mm on a 36 mm sensor, gliding along x.
bpy.ops.object.camera_add(location=(-0.5, -5, 0), rotation=(math.pi / 2, 0, 0))
cam = bpy.context.object; cam.name = "Shot"
cam.animation_data_create(); cam.animation_data.action = bpy.data.actions.new("ShotAction")
cam.data.lens = 50; cam.data.sensor_width = 36; cam.data.sensor_fit = "HORIZONTAL"
cam.keyframe_insert("location", frame=1); cam.location = (0.5, -5, 0); cam.keyframe_insert("location", frame=25)
bpy.ops.export_scene.fbx(filepath=sys.argv[-1], bake_anim=True, bake_anim_use_all_actions=False, bake_anim_use_nla_strips=False, add_leaf_bones=False)
print("exported")
