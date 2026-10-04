"""Original sculpted rally buggy and open roadster; no external model or texture.

Call ``build(globals())`` from build_assets.py. Coordinates are metres, Y up,
forward -Z. Bodies keep the shared gameplay seat and four animated axle pivots.
"""
import math

GROUPS = ('VehicleBody', 'VehicleWheel', 'VehicleSport')
DRIVER_HIP = (-.38, .89, .30)
WHEEL_CENTERS = tuple((x, .47, z) for x in (-1.02, 1.02) for z in (-1.02, 1.05))
WHEEL_RADIUS = .47


def build(api):
    bpy, bmesh = api['bpy'], api['bmesh']
    Matrix, Vector = api['Matrix'], api['Vector']
    begin, sphere, box = api['begin'], api['sphere'], api['box']
    cylinder, loft = api['cylinder'], api['loft']
    finish, material = api['finish_object'], api['material']
    paint = material('vehicle_pearl_paint', (.92, .95, .98), 6, True, .42)
    ivory = material('vehicle_ivory_stripe', 0xf0e5c3, 6, spec=.30)
    rubber = material('vehicle_rubber', 0x20292f, spec=.06)
    tread = material('vehicle_tread', 0x303b40, spec=.05)
    graphite = material('vehicle_graphite', 0x33434c, 6, spec=.28)
    satin = material('vehicle_brushed_alloy', 0xb4c2c7, 6, spec=.52)
    seat = material('vehicle_woven_seat', 0x384851, 11, spec=.08)
    seat_trim = material('vehicle_seat_insert', 0x718e90, 11, spec=.08)
    lens = material('vehicle_headlight', 0xf4e6bb, 9, spec=.48)
    tail = material('vehicle_tail_light', 0xe85649, 9, spec=.36)
    amber = material('vehicle_suspension_gold', 0xd6a843, 6, spec=.42)

    def surface(vertices, faces, mat, smooth=True):
        mesh = bpy.data.meshes.new('vehicle_sculpted_surface')
        mesh.from_pydata(vertices, [], faces)
        mesh.update()
        bm = bmesh.new()
        bm.from_mesh(mesh)
        bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
        bm.to_mesh(mesh)
        bm.free()
        obj = bpy.data.objects.new('vehicle_surface', mesh)
        bpy.context.collection.objects.link(obj)
        return finish(obj, mat, smooth=smooth)

    def hull(sections, mat, sides=12):
        """Superellipse sections loft along Z, with a broad shoulder and rounded chines."""
        vertices, faces = [], []
        for z, width, low, high in sections:
            for i in range(sides):
                angle = math.tau * i / sides
                c, s = math.cos(angle), math.sin(angle)
                x = math.copysign(abs(c) ** .58, c) * width
                y = (low + high) / 2 + math.copysign(abs(s) ** .65, s) * (high - low) / 2
                vertices.append((x, y, z))
        for ring in range(len(sections) - 1):
            for i in range(sides):
                j = (i + 1) % sides
                faces.append((ring*sides+i, ring*sides+j, (ring+1)*sides+j, (ring+1)*sides+i))
        faces += [tuple(reversed(range(sides))), tuple((len(sections)-1)*sides+i for i in range(sides))]
        return surface(vertices, faces, mat)

    def tube(points, radius, mat, sides=8):
        vertices, faces = [], []
        points = [Vector(p) for p in points]
        for k, point in enumerate(points):
            tangent = (points[min(k+1, len(points)-1)] - points[max(0, k-1)]).normalized()
            reference = Vector((1, 0, 0)) if abs(tangent.x) < .85 else Vector((0, 1, 0))
            u = tangent.cross(reference).normalized()
            v = tangent.cross(u).normalized()
            for i in range(sides):
                a = math.tau * i / sides
                vertices.append(tuple(point + radius * (u * math.cos(a) + v * math.sin(a))))
        for k in range(len(points)-1):
            for i in range(sides):
                j = (i+1) % sides
                faces.append((k*sides+i, k*sides+j, (k+1)*sides+j, (k+1)*sides+i))
        faces += [tuple(reversed(range(sides))), tuple((len(points)-1)*sides+i for i in range(sides))]
        return surface(vertices, faces, mat)

    def spring(x, z):
        points = [(x+.055*math.cos(i*math.tau*3.5/30), .51+i*.30/30,
                   z+.055*math.sin(i*math.tau*3.5/30)) for i in range(31)]
        tube(points, .011, satin, 6)
        cylinder((x, .46, z), (x, .87, z), .021, .021, amber, 8)

    def fender(x, z, mat):
        # A swept wheel arch with a rolled edge, hollow underneath the mudguard.
        cross = [(x-.16, .54), (x-.12, .57), (x+.12, .57), (x+.16, .54), (x+.15, .52), (x-.15, .52)]
        vertices, faces = [], []
        for k in range(13):
            a = -1.33 + 2.66*k/12
            for xx, radius in cross:
                vertices.append((xx, .47+radius*math.cos(a), z+radius*math.sin(a)))
        for k in range(12):
            for i in range(6):
                j = (i+1) % 6
                faces.append((k*6+i, k*6+j, (k+1)*6+j, (k+1)*6+i))
        faces += [tuple(reversed(range(6))), tuple(12*6+i for i in range(6))]
        return surface(vertices, faces, mat)

    def translated_loft(profile, x, mat, segments=12):
        obj = loft(profile, mat, segments)
        obj.matrix_world = Matrix.Translation(api['TO_BLENDER'] @ Vector((x, 0, 0))) @ obj.matrix_world
        return obj

    def cabin():
        # Seats sit around the unchanged player pelvis (.89 m); no roof or opaque windshield.
        for x in (-.38, .38):
            box((x,.49,.32),(.31,.20,.38),graphite,.024)
            translated_loft([(.61,.235,.25,.31), (.65,.29,.32,.30),
                             (.72,.28,.31,.31), (.75,.24,.27,.32)], x, seat)
            translated_loft([(.74,.25,.075,.62), (1.03,.27,.10,.68),
                             (1.29,.245,.085,.73), (1.42,.18,.06,.75)], x, seat)
            translated_loft([(.90,.20,.027,.555), (1.18,.21,.028,.605),
                             (1.33,.145,.024,.646)], x, seat_trim, 10)
            tube([(x-.17,1.36,.68), (x-.14,1.11,.585), (x-.14,.91,.50)], .016, ivory, 6)
        hull([(-.58,.75,.94,1.09), (-.43,.77,.97,1.11), (-.37,.67,1.0,1.08)], graphite)
        # An open, bent steering rim and two spokes aligned with the seated hands.
        points = [(-.38+.20*math.cos(i*math.tau/16),
                   1.16+.10*math.sin(i*math.tau/16),
                   -.30+.17*math.sin(i*math.tau/16)) for i in range(17)]
        tube(points, .022, rubber, 6)
        cylinder((-.38,1.16,-.30),(-.58,1.16,-.30),.012,.012,satin,6)
        cylinder((-.38,1.16,-.30),(-.18,1.16,-.30),.012,.012,satin,6)
        cylinder((-.38,1.15,-.31),(-.38,1.04,-.43),.033,.027,graphite,8)
        cylinder((.10,.72,.34),(.10,.99,.21),.014,.014,satin,6)
        sphere((.10,1.00,.20),(.032,.036,.032),rubber,10,6)

    def chassis(sport=False):
        hull([(-1.57,.70,.37,.50), (-1.22,.86,.36,.54),
              (-.59,.84,.32,.39), (.77,.86,.32,.39),
              (.97,.88,.36,.55), (1.52,.70,.39,.53)], graphite)
        for x in (-.80, .80):
            tube([(x,.53,-1.24), (x,.42,-.61), (x,.42,.68), (x,.56,1.20)], .038, satin, 8)
            for z in (-1.02,1.05):
                cylinder((x*.60,.48,z-.15), (math.copysign(1.03,x),.47,z), .021,.021,satin,8)
                cylinder((x*.60,.48,z+.15), (math.copysign(1.03,x),.47,z), .021,.021,satin,8)
            if not sport:
                spring(x, -1.02)

    begin('VehicleBody')
    chassis()
    hull([(-1.62,.56,.55,.74), (-1.46,.77,.55,.83), (-1.08,.85,.60,.94),
          (-.68,.78,.70,.96), (-.57,.67,.72,.92)], paint)
    hull([(.89,.69,.57,.90), (1.10,.85,.53,.93), (1.42,.78,.56,.85),
          (1.58,.61,.57,.76)], paint)
    # The cockpit is genuinely open; two sculpted side sill panels form its tub.
    for s in (-1,1):
        points = [(s*.80,.59,-.62),(s*.83,.83,-.53),(s*.86,.78,.71),
                  (s*.78,.58,.94),(s*.66,.50,.71),(s*.67,.49,-.52)]
        surface(points, [(0,1,2,3,4,5)], paint, False)
        tube([(s*.77,.55,-.52),(s*.88,.53,-.24),(s*.89,.54,.59),(s*.79,.61,.83)],.031,graphite)
        tube([(s*.76,.83,.77),(s*.76,1.82,.68),(s*.72,2.02,.49),
              (s*.70,2.04,-.17),(s*.71,1.87,-.45),(s*.76,1.07,-.67)],.037,graphite)
        tube([(s*.75,1.71,.72),(s*.86,1.24,1.02),(s*.81,.90,1.30)],.032,graphite)
        fender(s*1.0,-1.02,paint)
        fender(s*1.0,1.05,paint)
        # Recessed oval headlights and rear lamps have trim, lens and a tiny catchlight.
        cylinder((s*.55,.77,-1.52),(s*.55,.77,-1.64),.121,.105,graphite,12)
        sphere((s*.55,.77,-1.65),(.085,.072,.023),lens,12,6)
        sphere((s*.58,.80,-1.67),(.021,.013,.007),ivory,8,4)
        sphere((s*.58,.75,1.52),(.16,.045,.030),tail,12,6)
    tube([(-.73,2.01,.52),(0,2.06,.52),(.73,2.01,.52)],.037,graphite)
    tube([(-.76,.55,-1.69),(-.57,.50,-1.77),(.57,.50,-1.77),(.76,.55,-1.69)],.044,graphite)
    tube([(-.73,.55,1.61),(-.53,.50,1.68),(.53,.50,1.68),(.73,.55,1.61)],.038,graphite)
    # Central racing inlay follows the nose and catches light independently of body tint.
    surface([(-.055,.775,-1.57),(.055,.775,-1.57),(.073,.947,-1.08),
             (.058,.963,-.70),(-.058,.963,-.70),(-.073,.947,-1.08)],[(0,1,2,3,4,5)],ivory,False)
    for x in (-.22,-.11,0,.11,.22):
        cylinder((x,.59,-1.69),(x,.70,-1.68),.013,.013,satin,6)
    for x in (-.35,-.12,.12,.35):
        box((x,.917,1.04),(.068,.018,.28),rubber,0)
    cylinder((.66,.47,1.19),(.66,.52,1.65),.055,.068,graphite,10)
    cylinder((.66,.52,1.65),(.66,.52,1.70),.053,.055,satin,10)
    cabin()

    begin('VehicleSport')
    chassis(True)
    hull([(-1.76,.62,.45,.66),(-1.57,.87,.43,.73),(-1.17,.96,.47,.83),
          (-.76,.88,.52,.94),(-.56,.69,.66,.97)],paint,16)
    hull([(.81,.70,.60,.88),(1.0,.96,.46,.92),(1.35,.92,.43,.86),
          (1.65,.77,.49,.79)],paint,16)
    for s in (-1,1):
        # Swept flanks and pronounced haunches, with recessed air scoops.
        surface([(s*.68,.56,-.60),(s*.86,.76,-.57),(s*.93,.72,.66),
                 (s*.84,.55,.94),(s*.79,.44,.58),(s*.72,.45,-.48)],[(0,1,2,3,4,5)],paint,False)
        surface([(s*.858,.65,.21),(s*.920,.68,.68),(s*.842,.55,.75),
                 (s*.810,.52,.28)],[(0,1,2,3)],rubber,False)
        tube([(s*.69,1.04,.74),(s*.68,1.55,.75),(s*.55,1.69,.69),
              (s*.22,1.69,.69),(s*.15,1.53,.72),(s*.18,1.08,.69)],.035,graphite)
        fender(s*.99,-1.02,paint)
        fender(s*.99,1.05,paint)
        sphere((s*.64,.695,-1.655),(.20,.055,.033),graphite,12,6)
        sphere((s*.64,.703,-1.681),(.162,.027,.013),lens,12,6)
        sphere((s*.65,.729,1.58),(.24,.032,.024),tail,12,6)
        cylinder((s*.69,.45,1.51),(s*.69,.47,1.70),.075,.070,satin,12)
        cylinder((s*.69,.47,1.701),(s*.69,.47,1.708),.055,.055,rubber,10)
    # A shaped ducktail with two narrow stanchions, never over the driver's head.
    tube([(-.86,1.11,1.29),(-.65,1.19,1.32),(0,1.20,1.31),
          (.65,1.19,1.32),(.86,1.11,1.29)],.073,paint,10)
    cylinder((-.57,.84,1.24),(-.57,1.13,1.31),.018,.018,graphite,6)
    cylinder((.57,.84,1.24),(.57,1.13,1.31),.018,.018,graphite,6)
    hull([(-1.79,.39,.48,.585),(-1.63,.47,.47,.59)],rubber,12)
    for x in (-.20,-.10,0,.10,.20):
        cylinder((x,.505,-1.802),(x,.565,-1.802),.010,.010,satin,6)
    surface([(-.10,.738,-1.54),(.10,.738,-1.54),(.085,.842,-1.13),
             (.063,.946,-.74),(-.063,.946,-.74),(-.085,.842,-1.13)],[(0,1,2,3,4,5)],ivory,False)
    for s in (-1,1):
        cylinder((s*.61,.985,-.50),(s*.61,1.14,-.60),.012,.012,graphite,6)
        sphere((s*.62,1.17,-.605),(.095,.035,.060),paint,12,6)
    cabin()

    begin('VehicleWheel')
    # A hollow profiled tyre around the local X axle; unlike spheres or cylinders,
    # the crown, sidewall, bead and recessed alloy hub have separate contours.
    profile = [(-.14,.275),(-.198,.31),(-.195,.385),(-.157,.444),
               (-.095,.462),(.095,.462),(.157,.444),(.195,.385),(.198,.31),(.14,.275)]
    segments = 28
    vertices, faces = [], []
    for x, radius in profile:
        for i in range(segments):
            a = math.tau*i/segments
            vertices.append((x,radius*math.cos(a),radius*math.sin(a)))
    for ring in range(len(profile)):
        next_ring = (ring+1) % len(profile)
        for i in range(segments):
            j = (i+1) % segments
            faces.append((ring*segments+i,ring*segments+j,next_ring*segments+j,next_ring*segments+i))
    surface(vertices,faces,rubber)
    # Staggered tread blocks have beveled silhouettes rather than rectangular cubes.
    for k in range(24):
        a = math.tau*k/24
        for side in (-1,1):
            phase = a + side*.045
            vertices = []
            for radius in (.456,.473):
                for x, offset in [(side*.025,-.076),(side*.145,-.032),
                                  (side*.145,.045),(side*.025,.076)]:
                    angle = phase+offset
                    vertices.append((x,radius*math.cos(angle),radius*math.sin(angle)))
            surface(vertices,[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),
                              (2,3,7,6),(3,0,4,7)],tread,False)
    for side in (-1,1):
        x = side*.184
        cylinder((side*.128,0,0),(side*.179,0,0),.278,.291,satin,20)
        cylinder((side*.181,0,0),(side*.183,0,0),.232,.232,graphite,20)
        # Five sculpted fan spokes flare toward the rim and leave dark recesses.
        for k in range(5):
            a = math.tau*k/5
            def spoke_point(xx, radius, offset):
                angle = a+offset
                return (xx,radius*math.cos(angle),radius*math.sin(angle))
            vertices = [spoke_point(x,.070,-.23),spoke_point(x,.244,-.14),
                        spoke_point(x,.253,.15),spoke_point(x,.080,.28),
                        spoke_point(x+side*.022,.072,-.19),spoke_point(x+side*.018,.247,-.10),
                        spoke_point(x+side*.018,.25,.11),spoke_point(x+side*.022,.079,.24)]
            surface(vertices,[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),
                              (2,3,7,6),(3,0,4,7)],satin,False)
        cylinder((side*.188,0,0),(side*.215,0,0),.083,.077,graphite,12)
        cylinder((side*.216,0,0),(side*.222,0,0),.046,.046,amber,10)
        for k in range(5):
            a = math.tau*k/5
            cylinder((side*.213,.059*math.cos(a),.059*math.sin(a)),
                     (side*.225,.059*math.cos(a),.059*math.sin(a)),.009,.009,satin,6)

    return GROUPS
