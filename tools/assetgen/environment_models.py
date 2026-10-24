"""Original, zero-cost island props. Keep the existing gameplay footprints."""
import math
import random

GROUPS = ('Rock0', 'Rock1', 'Rock2', 'Boulder', 'Stump', 'HayBale', 'Barrel', 'Crate')


def build(api):
    begin, box, cyl = api['begin'], api['box'], api['cylinder']
    material, sphere = api['material'], api['sphere']
    bpy, Vector, Matrix = api['bpy'], api['Vector'], api['Matrix']
    finish = api['finish_object']
    wood = material('crate_honey_wood', 0xb88a52, 3)
    light = material('crate_end_grain', 0xd5ad72, 3)
    dark = material('crate_edges', 0x785238, 3)
    steel = material('prop_satin_steel', 0x65777f, 6, spec=.45)
    blue = material('barrel_cobalt', 0x337fa4, 6, spec=.45)
    yellow = material('hazard_ochre', 0xf5c356)
    black = material('prop_charcoal', 0x293b42)
    begin('Crate')
    for i in range(5):
        p = -.4 + i * .2
        for z in (-.465, .465):
            box((p,.50,z),(.19,.94,.07),wood,.009)
        for x in (-.465, .465):
            box((x,.50,p),(.07,.94,.19),wood,.009)
        box((p,1.0,0),(.19,.07,.99),light,.009)
    for x in (-.46,.46):
        for z in (-.46,.46):
            box((x,.51,z),(.12,1.02,.12),dark,.012)
    for z in (-.515,.515):
        for y in (.12,.89):
            box((0,y,z),(1,.10,.045),light,.01)
        brace = box((0,.5,z),(1.12,.09,.05),light,.01)
        brace.matrix_world @= Matrix.Rotation(.69,4,'Z')
        for x in (-.43,.43):
            for y in (.12,.89):
                cyl((x,y,z-.025),(x,y,z+.025),.018,.018,steel,8)

    begin('Barrel')
    cyl((0,.045,0),(0,.98,0),.412,.412,blue,20)
    for y in (.06,.28,.73,.98):
        cyl((0,y-.025,0),(0,y+.025,0),.432,.432,steel,20)
    cyl((0,.98,0),(0,1.005,0),.385,.385,black,20)
    cyl((0,1.005,0),(0,1.012,0),.365,.365,blue,20)
    for x,z,r in ((.19,.08,.06),(-.16,-.13,.035)):
        cyl((x,1.01,z),(x,1.04,z),r,r,steel,10)
    box((0,.5,-.414),(.24,.25,.014),yellow,.012)
    for x in (-.07,0,.07):
        box((x,.5,-.426),(.026,.17,.006),black,.002)

    straw = material('hay_gold', 0xd8b65b)
    cut = material('hay_cut_ends', 0xefd38a)
    rope = material('hay_twine', 0x8b7042)
    begin('HayBale')
    cyl((-.60,.62,0),(.60,.62,0),.62,.62,straw,20)
    for x in (-.31,.31):
        cyl((x-.018,.62,0),(x+.018,.62,0),.633,.633,rope,20)
    for side in (-1,1):
        for i,r in enumerate((.58,.49,.40,.31,.22,.13)):
            x = side * (.602+i*.003)
            cyl((x,.62,0),(x+side*.003,.62,0),r,r,cut if i%2==0 else straw,20)
    for i in range(16):
        a=i*math.tau/16
        cyl((-.58,.62+.615*math.cos(a),.615*math.sin(a)),
            (.58,.62+.615*math.cos(a),.615*math.sin(a)),.008,.008,cut,5)

    bark = material('stump_bark', 0x73533d,15)
    begin('Stump')
    cyl((0,0,0),(.025,.69,0),.43,.34,bark,12)
    for i in range(7):
        a=i*math.tau/7
        cyl((0,.35,0),(.57*math.cos(a),.055,.57*math.sin(a)),.15,.04,bark,7)
    for i,r in enumerate((.335,.275,.22,.155,.085)):
        cyl((.025,.69+i*.003,0),(.025,.696+i*.003,0),r,r,light if i%2==0 else wood,18)

    stone = material('island_stone_tint', 0x999da5,12,True)
    edge = material('island_stone_highlight', 0xb2b4b9,12,True)
    moss = material('island_moss', 0x739c50)
    for k,name in enumerate(GROUPS[:4]):
        begin(name)
        rng=random.Random(532+k)
        s=2 if name=='Boulder' else 1
        def rock(pos, scale, mat):
            bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2,radius=1)
            obj=bpy.context.object
            for v in obj.data.vertices:
                v.co *= rng.uniform(.86,1.12)
                v.co = Vector(tuple(v.co[j]*scale[j] for j in range(3)))
            finish(obj,mat,pos,smooth=False)
        rock((0,.37*s,0),(1*s,.72*s,.88*s),stone)
        rock((-.69*s,.20*s,.35*s),(.44*s,.33*s,.39*s),edge)
        rock((.13*s,.1*s,-.85*s),(.23*s,.18*s,.21*s),stone)
        if k>=2:
            rock((.68*s,.18*s,.30*s),(.48*s,.36*s,.46*s),stone)
        # Moss remains green when stone/ore recoloring is applied by the game.
        sphere((0,.97*s,0),(.54*s,.12*s,.45*s),moss,12,6)
    # One bevel segment gives crisp toy-like edges within the WebGPU budget.
    for name in GROUPS:
        for obj in api['groups'][name]:
            for modifier in obj.modifiers:
                if modifier.type == 'BEVEL':
                    modifier.segments = 1
