"""Original cartoon weapon models, authored in Blender without paid assets.

The right-hand grip and muzzle positions are the existing runtime IK contract.
All barrels point along -Z; these are game art, not manufacturing geometry.
"""
import math
import bpy
import bmesh


def build(api):
    begin = api['begin']
    base_box = api['box']
    sphere = api['sphere']
    cylinder = api['cylinder']
    finish = api['finish_object']
    material = api['material']
    gun, steel, black = api['GUN'], api['STEEL'], api['BLACK']
    dark, wood, copper = api['DARK'], api['WOOD'], api['COPPER']
    gold, blue, white = api['GOLD'], api['BLUE'], api['WHITE']
    walnut = material('weapon_walnut', 0xb57143, 3)
    grain = material('wood_inlay', 0x754732, 3)
    edge = material('weapon_edge', 0x657b85, 6, spec=.4)
    olive = material('launcher_olive', 0x708052, 6, spec=.25)
    orange = material('weapon_safety_orange', 0xf19745, 11)
    ammo_green = material('ammo_case_green', 0x66794b, 11)

    def box(pos, size, mat, bevel=.015):
        # One broad bevel is both more legible at game distance and much lighter
        # than many small highlight bands on each metal panel.
        obj = base_box(pos, size, mat, bevel)
        for mod in obj.modifiers:
            if mod.type == 'BEVEL':
                mod.segments = 1
        return obj

    def profile(points, width, mat, bevel=.004, x=0):
        """Extrude a deliberately drawn side silhouette, preserving flat panels."""
        count = len(points)
        verts = [(xx, y, z) for xx in (x-width/2, x+width/2) for y, z in points]
        faces = [tuple(reversed(range(count))), tuple(range(count, count*2))]
        for i in range(count):
            j = (i+1) % count
            faces.append((i, j, j+count, i+count))
        mesh = bpy.data.meshes.new('drawn_weapon_profile')
        mesh.from_pydata(verts, [], faces)
        mesh.update()
        bm = bmesh.new()
        bm.from_mesh(mesh)
        bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
        bm.to_mesh(mesh)
        bm.free()
        obj = bpy.data.objects.new('drawn_weapon_profile', mesh)
        bpy.context.collection.objects.link(obj)
        finish(obj, mat, smooth=False)
        if bevel:
            mod = obj.modifiers.new('soft_profile_edges', 'BEVEL')
            mod.width = bevel
            mod.segments = 1
        return obj

    def ring(y, z, outer, inner, mat, depth=.012, segments=12, x=0):
        """Real open ring normal to the barrel: sights and readable muzzle lips."""
        verts, faces = [], []
        for zz, radius in ((z-depth/2, outer), (z-depth/2, inner),
                           (z+depth/2, outer), (z+depth/2, inner)):
            for i in range(segments):
                a = math.tau*i/segments
                verts.append((x+math.cos(a)*radius, y+math.sin(a)*radius, zz))
        for i in range(segments):
            j = (i+1) % segments
            for a, b in ((0, 1), (0, 2), (1, 3), (2, 3)):
                faces.append((a*segments+i, a*segments+j, b*segments+j, b*segments+i))
        mesh = bpy.data.meshes.new('open_weapon_ring')
        mesh.from_pydata(verts, [], faces)
        mesh.update()
        bm = bmesh.new()
        bm.from_mesh(mesh)
        bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
        bm.to_mesh(mesh)
        bm.free()
        obj = bpy.data.objects.new('open_weapon_ring', mesh)
        bpy.context.collection.objects.link(obj)
        return finish(obj, mat)

    def bore(y, tip, radius, mat=gun):
        ring(y, tip+.006, radius, radius*.62, mat)
        cylinder((0,y,tip+.018), (0,y,tip+.019), radius*.60, radius*.60, black, 12)

    def screw(x, y, z, mat=steel, radius=.004):
        cylinder((x-.001,y,z),(x+.001,y,z),radius,radius,mat,8)

    def grip(mat=dark, narrow=False):
        # Main palm centre remains exactly (0, -.075, .014).
        profile([(-.023,-.013),(-.025,.030),(-.145,.057),(-.151,.010),(-.092,-.008)],
                .045 if narrow else .050, mat, .006)
        for side in (-1, 1):
            for i in range(3):
                cylinder((side*.026,-.073-i*.020,-.006+i*.004),
                         (side*.026,-.081-i*.020,.032+i*.005),.002,.002,black,6)
            screw(side*.027,-.054,.011,radius=.003)

    def trigger_guard(front=-.083, back=-.004, y=-.036):
        # Open interior reads correctly instead of a solid trigger block.
        cylinder((0,y,front),(0,y-.031,front+.004),.006,.006,gun,8)
        cylinder((0,y-.031,front+.004),(0,y-.034,back),.006,.006,gun,8)
        cylinder((0,y-.034,back),(0,y,back+.004),.006,.006,gun,8)
        cylinder((0,y+.011,(front+back)/2),(0,y-.013,(front+back)/2+.012),.004,.004,steel,6)

    def iron_sights(front, rear, y=.099, width=.033):
        box((0,y-.008,rear),(width,.017,.029),gun,.003)
        for side in (-1, 1):
            box((side*width*.38,y+.008,rear),(width*.24,.020,.017),black,.002)
        box((0,y-.002,front),(.015,.038,.018),gun,.003)
        box((0,y+.019,front),(.006,.008,.008),orange,.001)

    # AK-47: curved magazine, wood furniture, stamped receiver, separate gas tube.
    begin('WpnAr')
    profile([(.072,.045),(.080,-.042),(.068,-.269),(.047,-.311),
             (-.026,-.294),(-.038,-.030),(-.021,.051)], .078, gun, .005)
    cylinder((0,.079,.005),(0,.079,-.282),.031,.027,gun,12)
    profile([(.045,.040),(.052,.137),(.065,.180),(.047,.240),
             (-.082,.240),(-.074,.180),(-.026,.122),(-.016,.040)], .079, walnut, .009)
    profile([(.044,.238),(.040,.256),(-.087,.256),(-.088,.238)],.086,dark,.004)
    for s in (-1, 1):
        cylinder((s*.041,.015,.113),(s*.041,-.020,.227),.003,.003,grain,6)
        screw(s*.044,.014,.237,steel)
    # A tapered, faceted lower handguard reads as carved wood rather than a box.
    profile([(.026,-.293),(.036,-.321),(.026,-.484),
             (-.031,-.499),(-.052,-.465),(-.055,-.332)], .079, walnut, .007)
    cylinder((0,.082,-.282),(0,.082,-.479),.026,.022,walnut,12)
    for s in (-1, 1):
        for z in (-.329,-.374,-.419,-.461):
            box((s*.040,-.010,z),(.004,.023,.025),grain,.001)
    cylinder((0,.028,-.467),(0,.028,-.757),.018,.015,steel,12)
    cylinder((0,.081,-.465),(0,.081,-.621),.018,.015,gun,10)
    profile([(.094,-.606),(.097,-.634),(.026,-.649),(.010,-.616)],.051,gun,.004)
    cylinder((0,.028,-.734),(0,.028,-.773),.023,.023,gun,12)
    bore(.028,-.780,.025,steel)
    for s in (-1, 1):
        box((s*.023,.028,-.751),(.004,.019,.017),black,.001)
    box((0,.074,-.687),(.028,.091,.026),gun,.004)
    ring(.126,-.685,.022,.014,gun,.010,10)
    box((0,.123,-.685),(.005,.024,.007),steel,.001)
    iron_sights(-.274,-.073,.113,.042)
    profile([(-.028,-.118),(-.031,-.186),(-.112,-.211),(-.203,-.264),
             (-.264,-.326),(-.292,-.282),(-.216,-.203),(-.147,-.156),
             (-.068,-.120)], .052, gun, .006)
    for s in (-1, 1):
        for offset in (-.017,.016):
            nodes=[(-.070,-.163+offset),(-.138,-.204+offset),
                   (-.206,-.255+offset),(-.265,-.306+offset)]
            for (ya,za),(yb,zb) in zip(nodes,nodes[1:]):
                cylinder((s*.029,ya,za),(s*.029,yb,zb),.003,.003,edge,6)
        screw(s*.042,.034,-.026)
        screw(s*.042,.010,-.234)
        box((s*.041,.001,-.086),(.004,.018,.053),black,.002)
    cylinder((.039,.046,-.109),(.075,.046,-.109),.006,.006,steel,8)
    sphere((.075,.046,-.109),(.014,.009,.012),gun,10,6)
    cylinder((-.043,.022,-.163),(-.043,-.007,-.200),.004,.004,steel,6)
    trigger_guard(-.085,-.009,-.030)
    grip(walnut)

    # Pistol: a distinct short slide, inset frame, ejection port and combat sights.
    begin('WpnPistol')
    profile([(.099,.022),(.100,-.191),(.076,-.231),(.027,-.231),
             (.019,-.171),(.015,.031)],.061,edge,.004)
    profile([(.016,.025),(.024,-.171),(-.018,-.174),(-.032,-.103),
             (-.027,.035)],.059,gun,.004)
    cylinder((0,.045,-.170),(0,.045,-.231),.016,.013,black,12)
    bore(.045,-.240,.018,gun)
    box((.032,.069,-.106),(.004,.035,.056),black,.003)
    box((.035,.062,-.106),(.002,.017,.039),steel,.001)
    for s in (-1,1):
        for z in (.000,-.018,-.037):
            cylinder((s*.032,.038,z),(s*.032,.087,z),.002,.002,black,6)
    iron_sights(-.197,.003,.114,.029)
    trigger_guard(-.098,-.003,-.024)
    grip(dark, True)
    box((0,-.151,.033),(.056,.014,.055),gun,.003)
    cylinder((-.033,-.007,.006),(-.038,-.007,.006),.006,.006,steel,8)

    # Compact SMG: polymer fore-end, skeleton stock and ribbed straight magazine.
    begin('WpnSmg')
    profile([(.092,.049),(.092,-.177),(.055,-.252),(-.011,-.242),
             (-.040,-.089),(-.029,.041)],.073,gun,.006)
    cylinder((0,.056,-.213),(0,.028,-.485),.020,.017,steel,12)
    profile([(.032,-.232),(.061,-.263),(.045,-.417),
             (-.037,-.413),(-.049,-.270)],.068,dark,.006)
    for s in (-1,1):
        for z in (-.284,-.324,-.363):
            box((s*.036,.002,z),(.005,.024,.023),black,.002)
        cylinder((s*.031,.062,.051),(s*.031,.045,.189),.009,.009,edge,8)
        cylinder((s*.031,-.008,.044),(s*.031,-.050,.185),.009,.009,gun,8)
    profile([(.050,.179),(.045,.209),(-.083,.211),(-.080,.179)],.077,dark,.006)
    cylinder((0,.028,-.464),(0,.028,-.502),.028,.028,gun,12)
    bore(.028,-.510,.029,edge)
    profile([(-.035,-.105),(-.035,-.156),(-.215,-.181),
             (-.228,-.140)],.050,edge,.005)
    for s in (-1,1):
        for y in (-.089,-.130,-.171):
            box((s*.027,y,-.145),(.003,.006,.025),gun,.001)
        box((s*.039,.034,-.094),(.004,.028,.046),black,.002)
        screw(s*.040,.045,-.035)
    box((0,-.226,-.162),(.055,.015,.055),dark,.003)
    for z in (-.055,-.103,-.149):
        box((0,.103,z),(.041,.018,.025),edge,.002)
    iron_sights(-.379,-.037,.113,.040)
    cylinder((.039,.070,-.165),(.063,.070,-.165),.006,.006,steel,8)
    trigger_guard(-.084,-.011,-.029)
    grip(dark)

    # Pump shotgun: long twin tubes, scalloped walnut stock and corrugated pump.
    begin('WpnShotgun')
    profile([(.087,.059),(.084,-.158),(.063,-.231),(-.017,-.224),
             (-.029,-.021),(-.003,.060)],.082,gun,.006)
    profile([(.047,.031),(.039,.126),(.064,.209),(.042,.310),
             (-.077,.308),(-.064,.203),(-.007,.130),(-.023,.035)],.079,walnut,.010)
    profile([(.043,.299),(.038,.326),(-.080,.327),(-.083,.299)],.086,dark,.004)
    for s in (-1,1):
        cylinder((s*.042,.016,.146),(s*.042,-.024,.281),.003,.003,grain,6)
        box((s*.044,.031,-.106),(.004,.038,.089),black,.004)
        box((s*.047,.026,-.090),(.002,.019,.044),steel,.001)
        screw(s*.044,.048,.010)
    cylinder((0,.040,-.206),(0,.040,-.796),.022,.020,steel,14)
    cylinder((0,-.023,-.168),(0,-.023,-.689),.023,.020,gun,12)
    cylinder((0,-.023,-.659),(0,-.023,-.691),.026,.026,edge,12)
    profile([(.012,-.251),(.018,-.447),(-.062,-.468),
             (-.078,-.430),(-.072,-.265)],.081,walnut,.007)
    for z in (-.279,-.309,-.339,-.369,-.399,-.429):
        profile([(.011,z+.005),(.011,z-.006),(-.069,z-.006),(-.069,z+.005)],
                .084,grain,.001)
    for z in (-.598,-.731):
        box((0,.006,z),(.046,.061,.017),gun,.003)
    bore(.040,-.810,.026,gun)
    box((0,.068,-.740),(.011,.029,.018),orange,.002)
    trigger_guard(-.095,-.016,-.028)
    grip(walnut)

    # Bolt-action sniper: sculpted stock, free barrel, open scope ends and bolt knob.
    begin('WpnSniper')
    profile([(.046,.057),(.025,.186),(.065,.237),(.043,.337),
             (-.084,.335),(-.084,.276),(-.032,.184),(-.028,.035)],.078,walnut,.010)
    profile([(.044,.324),(.042,.348),(-.087,.348),(-.087,.324)],.083,dark,.004)
    profile([(.027,.044),(.030,-.246),(.006,-.518),(-.043,-.532),
             (-.067,-.245),(-.047,-.011)],.075,walnut,.007)
    cylinder((0,.035,.029),(0,.035,-.313),.031,.029,gun,12)
    cylinder((0,.035,-.295),(0,.035,-1.043),.021,.013,steel,14)
    cylinder((0,.035,-1.022),(0,.035,-1.058),.026,.026,gun,12)
    bore(.035,-1.070,.027,edge)
    for s in (-1,1):
        cylinder((s*.039,-.008,-.291),(s*.039,-.018,-.473),.003,.003,grain,6)
        screw(s*.040,-.005,.278)
    cylinder((.027,.035,-.048),(.079,.036,-.040),.007,.007,steel,8)
    cylinder((.079,.036,-.040),(.087,-.020,-.027),.007,.007,steel,8)
    sphere((.087,-.020,-.027),(.018,.018,.018),dark,10,7)
    for z in (-.073,-.230):
        box((0,.089,z),(.049,.071,.033),gun,.004)
        ring(.159,z,.043,.034,gun,.024,12)
    cylinder((0,.159,.006),(0,.159,-.332),.029,.029,black,14)
    cylinder((0,.159,.045),(0,.159,-.013),.041,.029,gun,14)
    cylinder((0,.159,-.327),(0,.159,-.414),.030,.047,gun,14)
    ring(.159,.047,.044,.034,edge,.012,14)
    ring(.159,-.419,.049,.039,edge,.012,14)
    cylinder((0,.159,-.411),(0,.159,-.412),.038,.038,blue,14)
    cylinder((0,.159,.040),(0,.159,.041),.033,.033,blue,14)
    cylinder((0,.173,-.171),(0,.211,-.171),.023,.023,gun,10)
    cylinder((0,.209,-.171),(0,.219,-.171),.027,.027,edge,10)
    cylinder((.021,.159,-.171),(.052,.159,-.171),.017,.017,gun,10)
    cylinder((.050,.159,-.171),(.060,.159,-.171),.022,.022,edge,10)
    trigger_guard(-.083,-.011,-.034)
    grip(walnut)
    # A slim slung bipod keeps the silhouette legible from third-person distance.
    for s in (-1,1):
        cylinder((s*.024,-.014,-.647),(s*.028,-.030,-.471),.009,.008,gun,8)

    # Launcher: chunky olive tube, shoulder pad, flip sight, front grip and open lip.
    begin('WpnRocket')
    cylinder((0,.09,.179),(0,.09,-.758),.073,.080,olive,16)
    cylinder((0,.09,.129),(0,.09,.189),.090,.090,gun,16)
    ring(.09,.189,.094,.072,edge,.017,16)
    cylinder((0,.09,.179),(0,.09,.180),.070,.070,black,14)
    for z in (.074,-.259,-.695):
        cylinder((0,.09,z+.019),(0,.09,z-.019),.083,.083,gun,14)
    ring(.09,-.780,.094,.073,edge,.020,16)
    cylinder((0,.09,-.760),(0,.09,-.761),.071,.071,black,14)
    profile([(.015,.128),(.015,.242),(-.067,.257),(-.088,.183),(-.035,.127)],.106,dark,.009)
    box((0,.082,-.108),(.098,.153,.136),gun,.009)
    box((0,.176,-.146),(.035,.041,.162),edge,.005)
    profile([(.191,-.246),(.271,-.242),(.283,-.298),(.192,-.303)],.017,gun,.003)
    box((0,.257,-.274),(.023,.040,.020),orange,.003)
    profile([(.004,-.363),(-.150,-.361),(-.163,-.414),(-.001,-.411)],.054,dark,.005)
    for y in (-.048,-.078,-.108):
        box((0,y,-.413),(.059,.009,.009),gun,.001)
    trigger_guard(-.085,-.003,-.037)
    grip(dark)
    # Visible warm safety stripe, not logos or text requiring textures.
    for x in (-.043,.043):
        box((x,.147,-.562),(.016,.012,.090),orange,.002)

    # Harvesting tool: drawn swept metal head with a visibly separate wooden shaft.
    begin('Pickaxe')
    cylinder((0,-.20,0),(0,.719,0),.023,.030,walnut,12)
    cylinder((0,-.208,0),(0,-.176,0),.028,.028,steel,10)
    for y in (-.143,-.105,-.067,-.029):
        cylinder((0,y,0),(0,y+.025,0),.027,.027,dark,10)
    cylinder((0,.665,0),(0,.740,0),.045,.045,gun,12)
    # This head is an X/Y silhouette extruded through Z, made through the same
    # side-profile helper then rotated to retain deliberate hooked points.
    points=[(.057,.0),(.050,-.13),(-.009,-.32),(-.098,-.407),
            (-.054,-.309),(-.009,-.119),(-.010,.110),(-.046,.289),
            (-.110,.390),(-.011,.319),(.047,.134)]
    obj=profile(points,.074,steel,.006)
    # Engine profile lies in Y/Z; rotate Z into X and offset to the shaft top.
    from mathutils import Matrix, Vector
    transform=Matrix.Translation(Vector((0,.737,0))) @ Matrix.Rotation(math.pi/2,4,'Y')
    obj.matrix_world = api['TO_BLENDER'] @ transform
    box((0,.739,0),(.143,.105,.098),gun,.012)
    cylinder((0,.793,0),(0,.809,0),.027,.027,gold,10)

    # Ammo crate: inset lid, reinforced corners, open handle and readable rounds.
    begin('AmmoBox')
    box((0,.075,0),(.21,.15,.15),ammo_green,.011)
    box((0,.153,0),(.224,.025,.160),gun,.006)
    box((0,.154,0),(.170,.028,.112),ammo_green,.004)
    for s in (-1,1):
        box((s*.108,.075,0),(.011,.115,.156),edge,.004)
        cylinder((s*.058,.169,.036),(s*.058,.202,.036),.007,.007,gun,8)
    cylinder((-.058,.202,.036),(.058,.202,.036),.007,.007,gun,8)
    box((0,.097,-.079),(.027,.044,.011),gold,.003)
    box((0,.052,-.080),(.097,.021,.006),white,.002)
    for x in (-.050,0,.050):
        cylinder((x,.165,-.022),(x,.238,-.022),.015,.015,gold,10)
        cylinder((x,.238,-.022),(x,.262,-.022),.015,.003,copper,10)
        cylinder((x,.165,-.022),(x,.173,-.022),.018,.018,copper,10)
