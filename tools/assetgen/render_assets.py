"""Studio previews of the exact FNM buffers embedded in the game, not mockups.

blender -b --factory-startup -P tools/assetgen/render_assets.py -- characters
Boards: characters, weapons, vehicles. PNGs go to docs/screenshots/.
"""
from pathlib import Path
import math
import struct
import sys
import bpy
from mathutils import Matrix, Vector

ROOT = Path(__file__).resolve().parents[2]
MESHES = ROOT / 'assets' / 'cartoon' / 'meshes'
CONVERT = Matrix.Rotation(math.pi / 2, 4, 'X')
BOARD = sys.argv[sys.argv.index('--') + 1] if '--' in sys.argv else 'characters'
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)


def rgb(value):
    return tuple(((value >> s) & 255) / 255 for s in (16, 8, 0))


def paint(kind):
    name = 'RuntimeMaterial' + str(kind)
    if name in bpy.data.materials:
        return bpy.data.materials[name]
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    shader = mat.node_tree.nodes.get('Principled BSDF')
    shader.inputs['Roughness'].default_value = .47 if kind == 6 else .74
    shader.inputs['Metallic'].default_value = .2 if kind == 6 else 0
    colors = mat.node_tree.nodes.new('ShaderNodeVertexColor')
    colors.layer_name = 'RuntimeColor'
    mat.node_tree.links.new(colors.outputs['Color'], shader.inputs['Base Color'])
    return mat


def mesh(name, xf=None, tint=0xffffff):
    data = (MESHES / (name + '.fnmesh')).read_bytes()
    magic, count, index_count = struct.unpack_from('<4sII', data)
    assert magic == b'FNM1'
    vertices = [struct.unpack_from('<6f8B', data, 12 + i * 32) for i in range(count)]
    indices = struct.unpack_from('<' + 'I' * index_count, data, 12 + count * 32)
    geometry = bpy.data.meshes.new(name)
    geometry.from_pydata([v[:3] for v in vertices], [], [indices[i:i+3] for i in range(0, index_count, 3)])
    geometry.update()
    for face in geometry.polygons:
        face.use_smooth = True
    geometry.normals_split_custom_set_from_vertices([v[3:6] for v in vertices])
    colors = geometry.color_attributes.new(name='RuntimeColor', type='BYTE_COLOR', domain='POINT')
    tint = rgb(tint)
    for color, v in zip(colors.data, vertices):
        color.color_srgb = (*[v[6+k]/255 * (tint[k] if v[9] else 1) for k in range(3)], 1)
    kinds = sorted({v[11] for v in vertices})
    for kind in kinds:
        geometry.materials.append(paint(kind))
    for face in geometry.polygons:
        face.material_index = kinds.index(vertices[face.vertices[0]][11])
    obj = bpy.data.objects.new(name, geometry)
    bpy.context.collection.objects.link(obj)
    obj.matrix_world = CONVERT @ (xf if xf is not None else Matrix.Identity(4))
    return obj


def tr(position):
    return Matrix.Translation(Vector(position))


def character(x, palette, hair):
    shirt, accent, pants, skin, hair_color, bag = palette
    base = tr((x, 0, 0)) @ Matrix.Rotation(-.15, 4, 'Y')
    for name, y, color in [('CharTorso',.93,shirt),('CharTrim',.93,accent),('CharPelvis',.93,pants),('CharBackpack',.93,bag),('CharHead',1.505,skin),(hair,1.505,hair_color)]:
        mesh(name, base @ tr((0,y,0)), color)
    for side in (-1,1):
        for name, y, color in [('CharArmUp',1.43,shirt),('CharArmLow',1.14,shirt),('CharHand',.87,skin)]:
            mesh(name, base @ tr((side*.25,y,0)), color)
        for name, y, color in [('CharLegUp',.9,pants),('CharLegLow',.47,pants),('CharBoot',.07,0x4b4139)]:
            mesh(name, base @ tr((side*.105,y,0)), color)


if BOARD == 'characters':
    palettes = [(0x287e78,0xd7b44e,0x696d4b,0xc98f67,0x302820,0xb96140), (0x56715a,0xc99b65,0x38464c,0x9c6a45,0x242526,0xd2ac6b), (0x506d9b,0xe8dcc0,0x394657,0xe8b48a,0x704c29,0xc98242), (0xb54b57,0xe3bd78,0x343d4b,0xc98f67,0x302820,0x495c6b)]
    for i, palette in enumerate(palettes):
        character((i-1.5)*1.15, palette, ['Hair1','Hair2','Hair3','Hair1'][i])
    camera_pos, focus, scale = (3,9,3.7), (0,0,1.0), 5.75
elif BOARD == 'weapons':
    names = ['WpnAr','WpnShotgun','WpnSniper','WpnSmg','WpnPistol','WpnRocket']
    for i, name in enumerate(names):
        # Side-on profile with the muzzle facing left; two rows of three.
        mesh(name, tr(((i%3-1)*1.65, .82+(1-i//3)*.75, 0)) @ Matrix.Rotation(math.pi/2,4,'Y'))
    camera_pos, focus, scale = (1.5,8,5.5), (0,0,1.2), 5.4
elif BOARD == 'vehicles':
    for i, name in enumerate(['VehicleBody','VehicleSport']):
        base = tr(((i-.5)*3.0,0,0)) @ Matrix.Rotation(-.25,4,'Y')
        mesh(name, base, [0xf1b942,0x4aaeb7][i])
        for x in (-1.02,1.02):
            for z in (-1.02,1.05):
                mesh('VehicleWheel', base @ tr((x,.47,z)))
    camera_pos, focus, scale = (7,10,7), (0,0,1), 9.2
else:
    raise ValueError('choose characters, weapons or vehicles')

scene = bpy.context.scene
scene.render.engine = 'BLENDER_EEVEE'
scene.render.resolution_x, scene.render.resolution_y = 1800, 1100
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = 'PNG'
scene.world.use_nodes = True
scene.world.node_tree.nodes.get('Background').inputs[0].default_value = (.095,.12,.18,1)
scene.world.node_tree.nodes.get('Background').inputs[1].default_value = .45
scene.view_settings.view_transform = 'AgX'
bpy.ops.mesh.primitive_plane_add(size=200)
floor = bpy.context.object
floor.location.z = -.008
mat = bpy.data.materials.new('StudioFloor')
mat.diffuse_color = (.075,.10,.16,1)
floor.data.materials.append(mat)
for pos, power, size in [((1,5,8),1900,6),((-5,1,4),1300,5),((3,-4,6),2100,5)]:
    bpy.ops.object.light_add(type='AREA', location=pos)
    light = bpy.context.object
    light.data.energy, light.data.shape, light.data.size = power, 'DISK', size
    light.rotation_euler = (Vector(focus)-light.location).to_track_quat('-Z','Y').to_euler()
bpy.ops.object.camera_add(location=camera_pos)
camera = bpy.context.object
camera.rotation_euler = (Vector(focus)-camera.location).to_track_quat('-Z','Y').to_euler()
camera.data.type, camera.data.ortho_scale = 'ORTHO', scale
scene.camera = camera
scene.render.filepath = str(ROOT / 'docs' / 'screenshots' / ('models-' + BOARD + '.png'))
bpy.ops.render.render(write_still=True)
print('RENDERED', scene.render.filepath, flush=True)
