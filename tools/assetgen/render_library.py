"""Import the delivered GLBs into Blender and render them for visual QA.

blender -b --factory-startup -P tools/assetgen/render_library.py -- buildings|island
"""
from pathlib import Path
import math
import sys
import bpy
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[2]
BOARD=sys.argv[sys.argv.index('--')+1] if '--' in sys.argv else 'buildings'
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)

if BOARD=='buildings':
    paths=sorted((ROOT/'assets/generated/buildings').glob('*.glb'))
    assert len(paths)==17
    for i,path in enumerate(paths):
        before=set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=str(path))
        imported=set(bpy.data.objects)-before
        corners=[obj.matrix_world@Vector(v) for obj in imported if obj.type=='MESH' for v in obj.bound_box]
        lo=Vector(tuple(min(v[j] for v in corners) for j in range(3)))
        hi=Vector(tuple(max(v[j] for v in corners) for j in range(3)))
        scale=4.2/max(hi-lo)
        parent=bpy.data.objects.new(path.stem,None)
        bpy.context.collection.objects.link(parent)
        for obj in imported:
            if obj.parent is None: obj.parent=parent
        parent.scale=(scale,)*3
        parent.location=((i%5-2)*6-(hi.x+lo.x)*.5*scale,
                         (i//5)*6-(hi.y+lo.y)*.5*scale,-lo.z*scale)
        bpy.ops.object.text_add(location=((i%5-2)*6,(i//5)*6-2.8,.015))
        text=bpy.context.object
        text.data.body=path.stem
        text.data.align_x='CENTER'
        text.data.size=.33
    camera_pos,focus,scale=(23,-30,37),(0,8,0),45
    floor_size=200
elif BOARD=='island':
    bpy.ops.import_scene.gltf(filepath=str(ROOT/'assets/generated/map/island.glb'))
    camera_pos,focus,scale=(1400,-1800,2200),(0,0,0),2700
    floor_size=10000
else:
    raise ValueError('choose buildings or island')

scene=bpy.context.scene
scene.render.engine='BLENDER_EEVEE'
scene.render.resolution_x,scene.render.resolution_y=1800,1300
scene.render.resolution_percentage=100
scene.render.image_settings.file_format='PNG'
scene.world.use_nodes=True
scene.world.node_tree.nodes.get('Background').inputs[0].default_value=(.22,.3,.42,1)
scene.world.node_tree.nodes.get('Background').inputs[1].default_value=.65
scene.view_settings.view_transform='AgX'
bpy.ops.mesh.primitive_plane_add(size=floor_size,location=(0,0,-.08 if BOARD=='buildings' else -70))
mat=bpy.data.materials.new('DisplayGround')
mat.diffuse_color=(.13,.2,.25,1)
bpy.context.object.data.materials.append(mat)
bpy.ops.object.light_add(type='SUN',location=(0,0,100))
bpy.context.object.rotation_euler=(math.radians(28),math.radians(-25),math.radians(-28))
bpy.context.object.data.energy=2.5
bpy.context.object.data.angle=math.radians(20)
bpy.ops.object.camera_add(location=camera_pos)
camera=bpy.context.object
camera.rotation_euler=(Vector(focus)-camera.location).to_track_quat('-Z','Y').to_euler()
camera.data.type='ORTHO'
camera.data.ortho_scale=scale
camera.data.clip_end=10000
scene.camera=camera
scene.render.filepath=str(ROOT/'docs/screenshots'/('models-'+BOARD+'.png'))
bpy.ops.render.render(write_still=True)
print('RENDERED',scene.render.filepath,flush=True)
