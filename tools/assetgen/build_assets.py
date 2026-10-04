"""Author the free Fightnight cartoon asset pack in Blender, then export it.

Run: blender --background --factory-startup --python tools/assetgen/build_assets.py
No external services, downloaded models, textures or paid add-ons are used.
Engine convention: metres, Y up, forward -Z. Blender uses Z up.
"""
from pathlib import Path
import bpy
import bmesh
import json
import math
import random
import struct
from mathutils import Matrix, Vector

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'assets' / 'cartoon'
BIN = OUT / 'meshes'
OUT.mkdir(parents=True, exist_ok=True)
BIN.mkdir(exist_ok=True)
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
TO_BLENDER = Matrix.Rotation(math.pi / 2, 4, 'X')
TO_ENGINE = TO_BLENDER.inverted()
groups = {}
group = ''
materials = {}

def rgb(value):
    if isinstance(value, int):
        return tuple(((value >> s) & 255) / 255 for s in (16, 8, 0))
    return tuple(value)

def linear(c):
    return c / 12.92 if c <= .04045 else ((c + .055) / 1.055) ** 2.4

def material(name, color, kind=0, tint=False, spec=.15):
    if name in materials:
        return materials[name]
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    color = rgb(color)
    shader = m.node_tree.nodes.get('Principled BSDF')
    shader.inputs['Base Color'].default_value = (*map(linear, color), 1)
    shader.inputs['Roughness'].default_value = .8 if kind != 6 else .42
    if kind == 6:
        shader.inputs['Metallic'].default_value = .35
    m['engine_rgb'] = color
    m['engine_mat'] = kind
    m['engine_tint'] = tint
    m['engine_spec'] = spec
    materials[name] = m
    return m

CLOTH = material('cloth_recolor', (.94, .96, .96), 11, True)
TRIM = material('outfit_accent', (.98, .98, .94), 11, True)
PANTS = material('trouser_recolor', (.9, .93, .92), 11, True)
SKIN = material('skin_recolor', (.99, .98, .97), 10, True)
HAIR = material('hair_recolor', (.85, .82, .78), 0, True)
DARK = material('rubber_seams', 0x253034)
STRAP = material('woven_strap', 0x3b413a, 11)
STEEL = material('satin_steel', 0x929ba2, 6, spec=.6)
GUN = material('graphite_metal', 0x364750, 6, spec=.35)
COPPER = material('warm_leather', 0xbd633d, 11)
GOLD = material('gold_fittings', 0xdba943, 6, spec=.45)
WHITE = material('warm_white', 0xe9e4d4)
BLACK = material('soft_black', 0x172328)
WOOD = material('warm_wood', 0x946545, 3)
BARK = material('bark', 0x65523f, 15)
LEAF = material('foliage_recolor', (.8, .9, .8), 1, True)
BLUE = material('shield_liquid', 0x389fd9, 7, spec=.5)
RED = material('medical_red', 0xd55e4d, 11)

def begin(name):
    global group
    group = name
    groups.setdefault(name, [])

def finish_object(obj, mat, pos=(0, 0, 0), rotation=None, smooth=True):
    obj.name = group + '_' + str(len(groups[group]))
    obj.data.materials.append(mat)
    for p in obj.data.polygons:
        p.use_smooth = smooth
    transform = Matrix.Translation(Vector(pos))
    if rotation is not None:
        transform @= rotation
    obj.matrix_world = TO_BLENDER @ transform
    groups[group].append(obj)
    return obj

def sphere(pos, scale, mat, segments=16, rings=10):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=segments, ring_count=rings, radius=1)
    obj = bpy.context.object
    obj.data.transform(Matrix.Diagonal((*scale, 1)))
    return finish_object(obj, mat, pos)

def box(pos, size, mat, bevel=.015):
    bpy.ops.mesh.primitive_cube_add(size=1)
    obj = bpy.context.object
    obj.data.transform(Matrix.Diagonal((*size, 1)))
    finish_object(obj, mat, pos, smooth=False)
    if bevel:
        modifier = obj.modifiers.new('rounded_edge_highlights', 'BEVEL')
        modifier.width = min(bevel, min(size) * .24)
        modifier.segments = 2
    return obj

def cylinder(a, b, r0, r1, mat, segments=12):
    a, b = Vector(a), Vector(b)
    delta = b - a
    bpy.ops.mesh.primitive_cone_add(vertices=segments, radius1=r0, radius2=r1, depth=delta.length)
    obj = bpy.context.object
    rotation = Vector((0, 0, 1)).rotation_difference(delta.normalized()).to_matrix().to_4x4()
    return finish_object(obj, mat, (a + b) / 2, rotation)

def loft(profile, mat, segments=16):
    """Smooth elliptical rings give jackets and limbs a tailored silhouette."""
    verts, faces = [], []
    for y, rx, rz, z in profile:
        for i in range(segments):
            a = i * math.tau / segments
            verts.append((math.cos(a) * rx, y, math.sin(a) * rz + z))
    for k in range(len(profile) - 1):
        for i in range(segments):
            n = (i + 1) % segments
            faces.append((k*segments+i, k*segments+n, (k+1)*segments+n, (k+1)*segments+i))
    faces.append(tuple(reversed(range(segments))))
    faces.append(tuple((len(profile)-1)*segments+i for i in range(segments)))
    mesh = bpy.data.meshes.new(group)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    bm = bmesh.new()
    bm.from_mesh(mesh)
    bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(group, mesh)
    bpy.context.collection.objects.link(obj)
    return finish_object(obj, mat)

def character():
    begin('CharTorso')
    loft([(0,.153,.104,0),(.055,.165,.108,0),(.19,.182,.117,0),(.36,.221,.137,0),(.445,.217,.13,0),(.51,.125,.095,0)], CLOTH)
    for s in (-1,1):
        sphere((s*.218,.43,0),(.08,.10,.109), CLOTH)
        box((s*.10,.20,-.117),(.105,.078,.012), CLOTH, .014)
        box((s*.10,.232,-.127),(.096,.009,.008), STRAP, .002)
        cylinder((s*.155,.025,-.09),(s*.20,.34,-.10),.006,.006,STRAP,6)
    box((0,.29,-.139),(.009,.39,.009),DARK,.002)
    box((0,.427,-.15),(.017,.036,.012),STEEL,.004)
    begin('CharTrim')
    loft([(.405,.224,.14,0),(.46,.21,.129,0),(.507,.131,.099,0)], TRIM)
    for s in (-1,1):
        sphere((s*.232,.485,0),(.071,.04,.091),TRIM,12,8)
    cylinder((0,.5,0),(0,.548,0),.071,.066,STRAP)
    begin('CharPelvis')
    loft([(-.126,.171,.105,0),(-.075,.18,.111,0),(.015,.165,.108,0),(.055,.16,.107,0)],PANTS)
    loft([(.024,.171,.113,0),(.06,.168,.112,0)],DARK)
    box((0,.045,-.117),(.042,.037,.014),STEEL,.008)
    for s in (-1,1):
        box((s*.18,-.018,0),(.044,.1,.075),STRAP,.014)
    begin('CharHead')
    cylinder((0,-.045,0),(0,.055,0),.043,.049,SKIN)
    sphere((0,.128,.006),(.111,.145,.107),SKIN,20,14)
    sphere((0,.053,-.022),(.084,.077,.083),SKIN,16,10)
    sphere((0,.094,-.107),(.024,.034,.033),SKIN,12,8)
    for s in (-1,1):
        sphere((s*.108,.104,.002),(.021,.037,.018),SKIN,12,8)
        sphere((s*.041,.151,-.094),(.026,.014,.011),WHITE,12,8)
        sphere((s*.04,.15,-.104),(.009,.009,.0045),BLACK,10,6)
        brow=box((s*.042,.178,-.092),(.047,.009,.009),DARK,.003)
    box((0,.046,-.093),(.036,.005,.009),material('lips',0x986752),.002)
    for name, kind in [('Hair1','short'),('Hair2','long'),('Hair3','ponytail')]:
        begin(name)
        # overlapping swept tufts, with a clean forehead and sculpted sideburns
        sphere((0,.23,.009),(.116,.073,.11),HAIR,16,8)
        for i in range(5):
            sphere((-.085+i*.039,.247+(.018 if i<3 else 0),-.033+i*.012),(.038,.052,.076),HAIR,12,8)
        for s in (-1,1):
            sphere((s*.103,.178,.035),(.024,.052,.06),HAIR,12,8)
        if kind!='short':
            sphere((0,.14,.085),(.112,.123,.05),HAIR,16,10)
        if kind=='ponytail':
            sphere((0,.12,.14),(.045,.11,.054),HAIR,12,10)
            cylinder((0,.18,.11),(0,.18,.155),.03,.03,STRAP)
    begin('CharArmUp')
    loft([(0,.073,.074,0),(-.065,.078,.077,0),(-.18,.065,.066,0),(-.29,.055,.056,0)],CLOTH)
    begin('CharArmLow')
    loft([(0,.055,.057,0),(-.10,.062,.056,0),(-.23,.043,.043,0),(-.27,.041,.04,0)],CLOTH)
    cylinder((0,-.23,0),(0,-.27,0),.047,.047,STRAP)
    begin('CharHand')
    sphere((0,-.04,-.012),(.038,.061,.025),SKIN,12,8)
    sphere((.028,-.031,-.026),(.018,.029,.018),SKIN,10,8)
    for i in range(4):
        cylinder((-.026+i*.017,-.071,-.013),(-.026+i*.017,-.096,-.018),.009,.007,SKIN,8)
    begin('CharLegUp')
    loft([(0,.092,.098,0),(-.09,.102,.105,0),(-.23,.082,.087,0),(-.43,.065,.068,0)],PANTS)
    box((.084,-.19,.011),(.016,.15,.10),PANTS,.011)
    begin('CharLegLow')
    loft([(0,.065,.069,0),(-.08,.07,.076,0),(-.23,.060,.068,0),(-.4,.047,.052,0)],PANTS)
    box((0,-.03,-.068),(.09,.105,.025),STRAP,.014)
    begin('CharBoot')
    loft([(.055,.056,.06,0),(-.02,.064,.072,-.018),(-.08,.069,.098,-.035)],STRAP)
    box((0,-.053,-.046),(.14,.105,.255),material('boot_recolor',(.86,.87,.86),11,True),.025)
    box((0,-.076,-.046),(.147,.028,.264),DARK,.008)
    for y in (-.012,.009,.03):
        cylinder((-.045,y,-.105),(.045,y,-.105),.005,.005,WHITE,6)
    begin('CharBackpack')
    box((0,.288,.174),(.32,.36,.145),CLOTH,.043)
    box((0,.235,.264),(.245,.20,.059),CLOTH,.025)
    box((0,.37,.25),(.19,.108,.027),DARK,.01)
    box((0,.218,.299),(.17,.012,.01),STRAP,.003)
    box((.074,.201,.305),(.018,.036,.012),STEEL,.003)
    for s in (-1,1):
        cylinder((s*.115,.46,.12),(s*.123,.09,.126),.015,.015,STRAP,8)
        box((s*.124,.13,.245),(.026,.065,.025),STRAP,.005)
    cylinder((-.067,.476,.17),(.067,.476,.17),.015,.015,STRAP,8)

def weapons():
    profiles=[('WpnPistol',.24),('WpnSmg',.51),('WpnAr',.78),('WpnShotgun',.81),('WpnSniper',1.07),('WpnRocket',.79)]
    for name,length in profiles:
        begin(name)
        if name=='WpnRocket':
            cylinder((0,.09,.14),(0,.09,-length),.077,.084,GUN,16)
            cylinder((0,.09,-length+.01),(0,.09,-length+.08),.09,.09,STEEL,16)
            box((0,.21,-.25),(.08,.04,.14),COPPER,.01)
        else:
            barrel_y={'WpnPistol':.045,'WpnShotgun':.04,'WpnSniper':.035}.get(name,.028)
            body_len=min(length*.65,.40)
            box((0,.035,-body_len*.37),(.073,.12,body_len),GUN,.012)
            cylinder((0,barrel_y,-body_len*.78),(0,barrel_y,-length),.019,.015,STEEL)
            cylinder((0,barrel_y,-length+.03),(0,barrel_y,-length),.025,.021,GUN)
            if name!='WpnPistol':
                box((0,.037,.14),(.071,.105,.21),COPPER,.012)
                box((0,.035,.24),(.078,.127,.033),DARK,.008)
                box((0,-.003,-length*.45),(.063,.066,min(.26,length*.32)),COPPER,.013)
                for k in range(5):
                    box((0,.073,-.13-k*.035),(.078,.022,.014),DARK,.002)
                box((0,-.14,-.13),(.045,.19,.083),GUN,.014)
            if name=='WpnSniper':
                cylinder((0,.155,-.08),(0,.155,-.35),.031,.031,BLACK)
                cylinder((0,.155,-.32),(0,.155,-.365),.042,.042,STEEL)
                cylinder((0,.155,-.366),(0,.155,-.368),.031,.031,BLUE)
            else:
                box((0,.115,-.075),(.019,.025,.06),DARK,.003)
        box((0,-.075,.014),(.048,.13,.055),STRAP,.009)
        cylinder((-.023,-.042,-.054),(.023,-.042,-.054),.005,.005,STEEL,6)
    begin('Pickaxe')
    cylinder((0,-.20,0),(0,.74,0),.022,.024,WOOD)
    for k in range(4):
        cylinder((0,-.13+k*.04,0),(0,-.11+k*.04,0),.025,.025,STRAP,10)
    box((0,.73,0),(.17,.11,.12),GUN,.012)
    cylinder((0,.74,0),(-.37,.66,0),.057,.007,STEEL)
    cylinder((0,.74,0),(.33,.65,0),.057,.01,STEEL)
    begin('AmmoBox')
    box((0,.065,0),(.19,.13,.13),material('ammo_box',0x657344,11),.012)
    box((0,.10,-.071),(.11,.046,.009),GOLD,.003)
    for x in (-.053,0,.053):
        cylinder((x,.13,0),(x,.23,0),.016,.016,GOLD)
        cylinder((x,.23,0),(x,.263,0),.016,.001,COPPER)

def supplies():
    for name,r,h in [('ItemShieldMini',.055,.14),('ItemShieldBig',.088,.21),('ItemChug',.104,.23)]:
        begin(name)
        cylinder((0,0,0),(0,h*.8,0),r,r,BLUE,16)
        sphere((0,h*.78,0),(r,r*.43,r),BLUE,16,8)
        cylinder((0,h*.78,0),(0,h,0),r*.35,r*.35,STEEL)
        cylinder((0,h-.015,0),(0,h+.02,0),r*.44,r*.44,DARK)
        for y in (.018,h*.56):
            cylinder((0,y,0),(0,y+.012,0),r*1.025,r*1.025,STEEL)
        box((0,h*.38,-r),(.045,.068,.009),WHITE,.004)
        box((0,h*.38,-r-.006),(.014,.041,.004),BLUE,.002)
    begin('ItemMedkit')
    box((0,.07,0),(.23,.14,.17),WHITE,.024)
    box((0,.107,-.01),(.245,.039,.178),RED,.007)
    box((0,.055,-.088),(.025,.071,.008),RED,.002)
    box((0,.055,-.09),(.078,.025,.008),RED,.002)
    begin('ItemBandage')
    cylinder((0,.033,-.05),(0,.033,.05),.036,.036,WHITE,16)
    cylinder((0,.033,-.051),(0,.033,-.052),.016,.016,DARK,12)
    box((0,.024,.034),(.063,.024,.06),WHITE,.003)
    for name,lid in [('ChestBase',False),('ChestLid',True)]:
        begin(name)
        if not lid:
            box((0,.23,0),(.90,.46,.60),WOOD,.035)
            for x in (-.285,.285):
                box((x,.23,-.31),(.049,.46,.019),GOLD,.006)
                box((x,.23,.31),(.049,.46,.019),GOLD,.006)
                for y in (.052,.35):
                    sphere((x,y,-.322),(.013,.013,.009),STEEL,8,6)
            box((0,.38,-.324),(.076,.115,.023),GOLD,.009)
            box((0,.37,-.34),(.018,.041,.009),DARK,.003)
        else:
            # Existing runtime translates this local 0..0.6 depth about the rear hinge.
            box((0,.055,.30),(.92,.115,.60),WOOD,.026)
            for x in (-.285,.285):
                box((x,.113,.30),(.049,.012,.60),GOLD,.003)

def foliage():
    rng=random.Random(414)
    for name,height in [('Pine0',9.2),('Oak0',7.6),('Birch0',7.7),('Bush0',1.25)]:
        begin(name)
        if name!='Bush0':
            trunk_r=.31 if name!='Birch0' else .18
            cylinder((0,0,0),(.08,height*.63,.03),trunk_r,trunk_r*.40,BARK,12)
            for k in range(4):
                a=k*math.tau/4+.3
                cylinder((0,height*.38,0),(math.cos(a)*1.2,height*.64,math.sin(a)*1.2),.12,.025,BARK,8)
        if name=='Pine0':
            for tier in range(7):
                y=1.7+tier*.94
                r=2.1*(1-tier*.108)
                for branch in range(5):
                    a=branch*math.tau/5+tier*.66
                    sphere((math.cos(a)*r*.50,y,math.sin(a)*r*.50),(r*.64,.90,r*.60),LEAF,10,6)
            sphere((0,8.65,0),(.52,.65,.52),LEAF,10,6)
        else:
            radius={'Oak0':2.8,'Birch0':1.65,'Bush0':1.05}[name]
            center={'Oak0':5.35,'Birch0':5.75,'Bush0':.56}[name]
            for k in range(14 if name!='Bush0' else 9):
                a=k*2.399963
                rr=radius*.72*math.sqrt((k+.5)/14)
                y=center+rng.uniform(-.65,.7)*radius*.48
                scale=radius*(.48 if name!='Bush0' else .52)
                sphere((math.cos(a)*rr,y,math.sin(a)*rr),(scale,scale*.85,scale),LEAF,12,7)
            top=height-(radius*.52)
            sphere((0,top,0),(radius*.57,radius*.52,radius*.57),LEAF,12,7)
    begin('GrassTuft')
    # Curved, tapered blades with asymmetric silhouettes instead of flat spikes.
    for k in range(11):
        a=rng.random()*math.tau
        h=rng.uniform(.22,.58)
        x,z=rng.uniform(-.19,.19),rng.uniform(-.19,.19)
        verts,faces=[],[]
        for j in range(5):
            t=j/4
            bend=h*.30*t*t
            width=.025*(1-t)*(.75+.25*math.sin(a))
            c=Vector((x+math.cos(a)*bend,h*t,z+math.sin(a)*bend))
            side=Vector((-math.sin(a)*width,0,math.cos(a)*width))
            verts.extend([tuple(c-side),tuple(c+side)])
        for j in range(4):
            faces.extend([(j*2,j*2+1,j*2+3,j*2+2),(j*2+2,j*2+3,j*2+1,j*2)])
        mesh=bpy.data.meshes.new('curved_blade')
        mesh.from_pydata(verts,[],faces)
        mesh.update()
        obj=bpy.data.objects.new('curved_blade',mesh)
        bpy.context.collection.objects.link(obj)
        finish_object(obj,material('grass_recolor',(.82,.94,.73),2,True),smooth=False)

def export_meshes():
    stats={}
    deps=bpy.context.evaluated_depsgraph_get()
    for name,objects in groups.items():
        vertices,indices,lookup=[],[],{}
        for obj in objects:
            evaluated=obj.evaluated_get(deps)
            mesh=evaluated.to_mesh()
            mesh.calc_loop_triangles()
            matrix=TO_ENGINE@evaluated.matrix_world
            normal_matrix=matrix.to_3x3().inverted().transposed()
            mat=obj.data.materials[0]
            color=tuple(round(c*255) for c in mat['engine_rgb'])
            kind=int(mat['engine_mat'])
            for triangle in mesh.loop_triangles:
                a,b,c=(matrix@mesh.vertices[i].co for i in triangle.vertices)
                if (b-a).cross(c-a).length_squared < 1e-14:
                    continue
                for index,loop in zip(triangle.vertices,triangle.loops):
                    p=matrix@mesh.vertices[index].co
                    n=(normal_matrix@mesh.corner_normals[loop].vector).normalized()
                    sway=min(.85,max(0,p.y*.09)) if kind==1 else min(1,max(0,p.y*1.6)) if kind==2 else 0
                    ao=.75+min(.25,max(0,p.y)*.035) if kind in (1,2) else .94
                    entry=struct.pack('<6f8B',*p,*n,*color,255 if mat['engine_tint'] else 0,round(ao*255),kind,round(sway*255),round(mat['engine_spec']*255))
                    if entry not in lookup:
                        lookup[entry]=len(vertices)
                        vertices.append(entry)
                    indices.append(lookup[entry])
            evaluated.to_mesh_clear()
        if len(vertices)>=5000:
            raise ValueError(f'{name} exceeds the established per-mesh vertex budget: {len(vertices)}')
        data=struct.pack('<4sII',b'FNM1',len(vertices),len(indices))+b''.join(vertices)+struct.pack('<'+'I'*len(indices),*indices)
        (BIN/(name+'.fnmesh')).write_bytes(data)
        stats[name]={'vertices':len(vertices),'triangles':len(indices)//3,'bytes':len(data)}
        print('MESH',name,stats[name],flush=True)
    return stats

def motion(phase):
    s,c=math.sin(phase),math.cos(phase)
    return (.022*(1-math.cos(2*phase)), .62*s*(.92 if s>0 else .76)+.045,
            .09+1.20*max(c,0)**1.7, .23*max(-s,0), -.07*s, .015*math.sin(phase))

def make_rig():
    """Rigid weighted segments match the runtime IK pivots; GLB contains a skeleton."""
    bpy.ops.object.armature_add()
    rig=bpy.context.object
    rig.name='Fightnight_Rig'
    bpy.ops.object.mode_set(mode='EDIT')
    rig.data.edit_bones.remove(rig.data.edit_bones[0])
    bindings={
        'pelvis':('CharPelvis',(0,.93,0),(0,1.02,0),None),
        'torso':('CharTorso',(0,.93,0),(0,1.43,0),'pelvis'),
        'head':('CharHead',(0,1.505,0),(0,1.76,0),'torso'),
        'backpack':('CharBackpack',(0,.93,0),(0,1.20,0),'torso'),
    }
    for side,x in [('L',-.25),('R',.25)]:
        bindings['upper_arm.'+side]=('CharArmUp',(x,1.43,0),(x,1.14,0),'torso')
        bindings['forearm.'+side]=('CharArmLow',(x,1.14,0),(x,.87,0),'upper_arm.'+side)
        bindings['hand.'+side]=('CharHand',(x,.87,0),(x,.78,0),'forearm.'+side)
        xx=-.105 if side=='L' else .105
        bindings['thigh.'+side]=('CharLegUp',(xx,.90,0),(xx,.47,0),'pelvis')
        bindings['shin.'+side]=('CharLegLow',(xx,.47,0),(xx,.07,0),'thigh.'+side)
        bindings['boot.'+side]=('CharBoot',(xx,.07,0),(xx,.07,-.14),'shin.'+side)
    for name,(_,head,tail,parent) in bindings.items():
        bone=rig.data.edit_bones.new(name)
        bone.head=TO_BLENDER@Vector(head)
        bone.tail=TO_BLENDER@Vector(tail)
        if parent:
            bone.parent=rig.data.edit_bones[parent]
    bpy.ops.object.mode_set(mode='OBJECT')
    objects=[]
    for bone_name,(mesh_name,head,_,_) in bindings.items():
        for original in groups[mesh_name]:
            obj=original.copy()
            obj.data=original.data.copy()
            bpy.context.collection.objects.link(obj)
            obj.parent=rig
            obj.hide_set(False);obj.hide_render=False
            obj.matrix_world=Matrix.Translation(TO_BLENDER@Vector(head))@original.matrix_world
            obj.name=bone_name+'_'+original.name
            obj.vertex_groups.new(name=bone_name).add(list(range(len(obj.data.vertices))),1,'REPLACE')
            arm=obj.modifiers.new('Rig','ARMATURE')
            arm.object=rig
            objects.append(obj)
    for mesh_name,bone_name,head in [('CharTrim','torso',(0,.93,0)),('Hair1','head',(0,1.505,0))]:
        for original in groups[mesh_name]:
            obj=original.copy();obj.data=original.data.copy()
            bpy.context.collection.objects.link(obj)
            obj.parent=rig
            obj.hide_set(False);obj.hide_render=False
            obj.matrix_world=Matrix.Translation(TO_BLENDER@Vector(head))@original.matrix_world
            obj.vertex_groups.new(name=bone_name).add(list(range(len(obj.data.vertices))),1,'REPLACE')
            obj.modifiers.new('Rig','ARMATURE').object=rig
            objects.append(obj)
    rig.animation_data_create()
    bpy.context.scene.render.fps=30
    for clip,length in [('Idle',60),('Walk',30),('Sprint',24),('Jump',36),('Crouch',36),('Reload',60),('Victory',60)]:
        rig.animation_data.action=None
        for frame in range(1,length+2,2):
            t=(frame-1)/length
            ph=t*math.tau
            for bone in rig.pose.bones:
                bone.rotation_mode='XYZ'
                bone.rotation_euler=(0,0,0)
                bone.location=(0,0,0)
            pelvis=rig.pose.bones['pelvis']; torso=rig.pose.bones['torso']
            if clip=='Idle':
                torso.rotation_euler.x=.015*math.sin(ph)
                rig.pose.bones['head'].rotation_euler.z=.025*math.sin(ph*.5)
            elif clip in ('Walk','Sprint'):
                amount=1.0 if clip=='Walk' else 1.18
                pelvis.location.y=-motion(ph)[0]*amount
                torso.rotation_euler.x=-.11 if clip=='Walk' else -.24
                for side,offset in [('L',0),('R',math.pi)]:
                    _,thigh,knee,foot,_,_=motion(ph+offset)
                    rig.pose.bones['thigh.'+side].rotation_euler.x=thigh*amount
                    rig.pose.bones['shin.'+side].rotation_euler.x=-knee
                    rig.pose.bones['boot.'+side].rotation_euler.x=foot
                    rig.pose.bones['upper_arm.'+side].rotation_euler.x=-thigh*.75
            elif clip=='Jump':
                pelvis.location.y=.48*math.sin(math.pi*t)
                for side in ('L','R'):
                    rig.pose.bones['thigh.'+side].rotation_euler.x=.7*math.sin(math.pi*t)
                    rig.pose.bones['shin.'+side].rotation_euler.x=-1.1*math.sin(math.pi*t)
                    rig.pose.bones['upper_arm.'+side].rotation_euler.x=.45*math.sin(math.pi*t)
            elif clip=='Crouch':
                k=min(1,t*3);k=k*k*(3-2*k)
                pelvis.location.y=-.30*k
                torso.rotation_euler.x=-.35*k+.012*math.sin(ph)
                for side in ('L','R'):
                    rig.pose.bones['thigh.'+side].rotation_euler.x=.95*k
                    rig.pose.bones['shin.'+side].rotation_euler.x=-1.9*k
            elif clip=='Reload':
                rig.pose.bones['upper_arm.R'].rotation_euler.x=.65
                rig.pose.bones['forearm.R'].rotation_euler.x=1.05
                rig.pose.bones['upper_arm.L'].rotation_euler.x=.35+.35*math.sin(ph)
                rig.pose.bones['forearm.L'].rotation_euler.x=1.2+.3*math.sin(ph)
            elif clip=='Victory':
                torso.rotation_euler.z=.14*math.sin(ph*2)
                for side,offset in [('L',0),('R',math.pi)]:
                    rig.pose.bones['upper_arm.'+side].rotation_euler.x=2.2+.5*math.sin(ph*2+offset)
                    rig.pose.bones['forearm.'+side].rotation_euler.x=.6
                    rig.pose.bones['thigh.'+side].rotation_euler.x=.35*max(0,math.sin(ph*2+offset))
            for bone in rig.pose.bones:
                bone.keyframe_insert(data_path='rotation_euler',frame=frame,group=bone.name)
                bone.keyframe_insert(data_path='location',frame=frame,group=bone.name)
        action=rig.animation_data.action
        action.name=clip
        action.use_fake_user=True
        track=rig.animation_data.nla_tracks.new()
        track.name=clip
        strip=track.strips.new(clip,1,action)
        strip.action_slot=rig.animation_data.action_slot
        track.mute=True
    rig.animation_data.action=None
    for bone in rig.pose.bones:
        bone.rotation_euler=(0,0,0);bone.location=(0,0,0)
    bpy.context.scene.frame_set(1)
    return rig,objects

def export_roster(rig,objects):
    palettes={
        'scout':{'CharTorso':0x287e78,'CharTrim':0xd7b44e,'CharPelvis':0x696d4b,'CharArmUp':0x287e78,'CharArmLow':0x287e78,'CharLegUp':0x696d4b,'CharLegLow':0x696d4b,'CharBackpack':0xb96140},
        'ranger':{'CharTorso':0x56715a,'CharTrim':0xc99b65,'CharPelvis':0x38464c,'CharBackpack':0xd2ac6b,'CharHead':0x9c6a45,'CharHand':0x9c6a45},
        'pilot':{'CharTorso':0x506d9b,'CharTrim':0xe8dcc0,'CharPelvis':0x394657,'CharBackpack':0xc98242,'CharHead':0xe8b48a,'CharHand':0xe8b48a,'Hair1':0x704c29},
        'vanguard':{'CharTorso':0xb54b57,'CharTrim':0xe3bd78,'CharPelvis':0x343d4b,'CharBackpack':0x495c6b},
    }
    for name in ['ranger','pilot','vanguard','scout']:
        palette=palettes[name]
        for obj in objects:
            base=obj.data.materials[0]
            if not base.get('engine_tint'):
                continue
            original_name=next((key for key in groups if key in obj.name),'CharTorso')
            fallback='CharTorso' if 'Arm' in original_name else 'CharPelvis' if 'Leg' in original_name else original_name
            value=palette.get(original_name,palette.get(fallback,0xc98f67 if original_name in ('CharHead','CharHand') else 0x302820 if 'Hair' in original_name else 0x4b4139))
            m=base.copy();m.name=name+'_'+original_name
            m.node_tree.nodes.get('Principled BSDF').inputs['Base Color'].default_value=(*map(linear,rgb(value)),1)
            obj.data.materials[0]=m
        bpy.ops.object.select_all(action='DESELECT')
        rig.select_set(True)
        for obj in objects:obj.select_set(True)
        bpy.context.view_layer.objects.active=rig
        bpy.ops.export_scene.gltf(filepath=str(OUT/(name+'.glb')),export_format='GLB',use_selection=True,export_animations=True,export_animation_mode='NLA_TRACKS',export_apply=True,export_lights=False,export_cameras=False)
    return list(palettes)

def export_libraries():
    """Named prop assemblies remain independently editable in Blender and glTF."""
    for pack,names,spacing in [('items',[n for n in groups if n.startswith(('Wpn','Item')) or n in ('Pickaxe','AmmoBox','ChestBase','ChestLid')],1.6),('foliage',['Pine0','Oak0','Birch0','Bush0','GrassTuft'],7.0)]:
        bpy.ops.object.select_all(action='DESELECT')
        for i,name in enumerate(names):
            parent=bpy.data.objects.new(name,None)
            bpy.context.collection.objects.link(parent)
            parent.location=TO_BLENDER@Vector((8+(i%5)*spacing,0,(i//5)*spacing))
            parent.select_set(True)
            for original in groups[name]:
                obj=original.copy();obj.data=original.data.copy()
                bpy.context.collection.objects.link(obj)
                obj.parent=parent
                obj.matrix_local=original.matrix_world
                obj.hide_set(False);obj.hide_render=False
                if pack=='foliage' and obj.data.materials[0].get('engine_tint'):
                    m=obj.data.materials[0].copy()
                    color=0x6ba449 if name=='GrassTuft' else 0x397b3f
                    m.node_tree.nodes.get('Principled BSDF').inputs['Base Color'].default_value=(*map(linear,rgb(color)),1)
                    obj.data.materials[0]=m
                obj.select_set(True)
        bpy.ops.export_scene.gltf(filepath=str(OUT/(pack+'.glb')),export_format='GLB',use_selection=True,export_apply=True,export_animations=False)

def main():
    character();weapons();supplies();foliage()
    stats=export_meshes()
    samples=[motion(i*math.tau/64) for i in range(64)]
    (OUT/'locomotion.fnmotion').write_bytes(struct.pack('<4sI',b'FNA1',64)+b''.join(struct.pack('<6f',*s) for s in samples))
    for objects in groups.values():
        for obj in objects:obj.hide_set(True);obj.hide_render=True
    rig,objects=make_rig()
    roster=export_roster(rig,objects)
    export_libraries()
    bpy.context.scene.world.color=(.08,.08,.08)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'fightnight-cartoon.blend'))
    manifest={'generator':'Blender '+bpy.app.version_string,'cost':0,'style':'original stylized cartoon battle royale','units':'metres, Y up, forward -Z','characters':roster,'animations':['Idle','Walk','Sprint','Jump','Crouch','Reload','Victory'],'meshes':stats}
    (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print('ASSET PACK COMPLETE',len(stats),'meshes',flush=True)

if __name__=='__main__':main()

