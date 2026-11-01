"""Package and render the Meshy powerwasher character with Blender.

blender -b --factory-startup --gpu-backend opengl -P tools/assetgen/package_powerwasher.py
"""
from pathlib import Path
import json
import math
import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'assets/characters/powerwasher'
SOURCE = OUT / 'powerwasher-meshy-source.glb'

bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
bpy.ops.import_scene.gltf(filepath=str(SOURCE))
models = list(bpy.context.scene.objects)
meshes = [obj for obj in models if obj.type == 'MESH']
corners = [obj.matrix_world @ Vector(v) for obj in meshes for v in obj.bound_box]
lo = Vector(tuple(min(p[i] for p in corners) for i in range(3)))
hi = Vector(tuple(max(p[i] for p in corners) for i in range(3)))
scale = 1.8 / (hi.z - lo.z)
offset = Vector(((hi.x + lo.x) / 2, (hi.y + lo.y) / 2, lo.z))
for obj in meshes:
    obj.data.transform(obj.matrix_world)
    obj.matrix_world.identity()
    for vertex in obj.data.vertices:
        vertex.co = (vertex.co - offset) * scale
    obj.name = 'PowerwasherHero'
    obj.data.name = 'PowerwasherHeroMesh'
    for polygon in obj.data.polygons:
        polygon.use_smooth = True
    obj['license'] = 'CC BY 4.0'
    obj['attribution'] = 'Powerwasher Hero by aldrinstori, generated using Meshy'
    obj['source'] = 'Meshy 6 Lite; original image reference generated with OpenAI'

scene = bpy.context.scene
scene.unit_settings.system = 'METRIC'
scene.unit_settings.scale_length = 1.0
for mat in bpy.data.materials:
    if mat.users:
        mat.name = 'Powerwasher_PBR'
bpy.ops.object.select_all(action='DESELECT')
for obj in meshes:
    obj.select_set(True)
bpy.context.view_layer.objects.active = meshes[0]
bpy.ops.export_scene.gltf(filepath=str(OUT / 'powerwasher.glb'), export_format='GLB', use_selection=True, export_extras=True, export_tangents=True)
bpy.ops.export_scene.fbx(filepath=str(OUT / 'powerwasher.fbx'), use_selection=True, object_types={'MESH'}, path_mode='COPY', embed_textures=True, axis_forward='-Z', axis_up='Y', bake_anim=False)
bpy.ops.wm.save_as_mainfile(filepath=str(OUT / 'powerwasher.blend'))

stats = {
    'height_metres': 1.8,
    'triangles': sum(len(obj.data.polygons) for obj in meshes),
    'vertices': sum(len(obj.data.vertices) for obj in meshes),
    'textures': [{'name': im.name, 'size': list(im.size)} for im in bpy.data.images if im.type == 'IMAGE'],
    'pose': 'A-pose',
    'geometry': 'AI-generated triangle mesh; not manual quad retopology',
    'handheld_props': False,
    'weapons': False,
}
(OUT / 'model-info.json').write_text(json.dumps(stats, indent=2) + '\n', encoding='utf-8')

# Same mesh from three orthographic views; no invented reverse-side painting.
for index, angle in enumerate((0, math.pi / 2, math.pi)):
    for source in meshes:
        obj = source if index == 0 else source.copy()
        if index:
            obj.data = source.data
            scene.collection.objects.link(obj)
        obj.rotation_mode = 'XYZ'
        obj.rotation_euler = (0, 0, angle)
        obj.location.x = (-1.65, 0, 1.65)[index]

scene.render.engine = 'BLENDER_EEVEE'
scene.render.resolution_x = 2560
scene.render.resolution_y = 1440
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = 'PNG'
scene.render.film_transparent = False
scene.world.use_nodes = True
scene.world.node_tree.nodes['Background'].inputs[0].default_value = (.48, .48, .48, 1)
scene.world.node_tree.nodes['Background'].inputs[1].default_value = .6
scene.view_settings.view_transform = 'AgX'

def area(name, position, power, size):
    bpy.ops.object.light_add(type='AREA', location=position)
    light = bpy.context.object
    light.name = name
    light.data.energy = power
    light.data.shape = 'DISK'
    light.data.size = size
    light.rotation_euler = (Vector((0, 0, .9)) - light.location).to_track_quat('-Z', 'Y').to_euler()

area('Softbox_Key', (-3, -4, 5), 500, 5)
area('Softbox_Fill', (4, -2, 3), 350, 4)
area('Softbox_Rim', (0, 3, 4), 450, 4)
bpy.ops.mesh.primitive_plane_add(size=200, location=(0, 0, -.015))
floor = bpy.context.object
floor.name = 'StudioFloor'
mat = bpy.data.materials.new('NeutralGrey')
mat.use_nodes = True
mat.node_tree.nodes['Principled BSDF'].inputs['Base Color'].default_value = (.38, .38, .38, 1)
mat.node_tree.nodes['Principled BSDF'].inputs['Roughness'].default_value = .85
floor.data.materials.append(mat)
bpy.ops.object.camera_add(location=(0, -10, .94))
camera = bpy.context.object
camera.rotation_euler = (math.pi / 2, 0, 0)
camera.data.type = 'ORTHO'
camera.data.ortho_scale = 5.2
scene.camera = camera
scene.render.filepath = str(OUT / 'turnaround.png')
bpy.ops.wm.save_as_mainfile(filepath=str(OUT / 'turnaround-studio.blend'))
bpy.ops.render.render(write_still=True)
print('POWERWASHER_PACKAGED', json.dumps(stats), flush=True)
