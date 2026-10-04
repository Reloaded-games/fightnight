"""Original, sculpted cartoon adventurers for Fightnight's segmented runtime rig.

``build(globals())`` is called by build_assets.py in place of its character().
All positions are local to the existing Y-up / -Z-forward segment pivots.
The small clothing assemblies remain rigid so gameplay weapon IK still works.
"""
import math


def build(api):
    bpy, bmesh = api['bpy'], api['bmesh']
    Vector = api['Vector']
    begin, finish = api['begin'], api['finish_object']
    box, cylinder, sphere = api['box'], api['cylinder'], api['sphere']
    material = api['material']
    cloth, accent, pants = api['CLOTH'], api['TRIM'], api['PANTS']
    skin, hair = api['SKIN'], api['HAIR']
    dark, strap, steel = api['DARK'], api['STRAP'], api['STEEL']
    white, black = api['WHITE'], api['BLACK']
    seam = material('outfit_woven_shadow', (.69, .75, .73), 11, True)
    cloth_hi = material('outfit_fold_highlight', (.99, .99, .98), 11, True)
    pant_shadow = material('trouser_fold_shadow', (.69, .74, .69), 11, True)
    skin_shadow = material('skin_sculpt_shadow', (.80, .66, .61), 10, True)
    lip = material('natural_lip_recolor', (.73, .49, .47), 10, True)
    hair_shadow = material('hair_sculpt_shadow', (.61, .57, .53), 0, True)
    hair_hi = material('hair_sculpt_highlight', (.99, .94, .87), 0, True)
    boot_mat = material('boot_recolor', (.89, .91, .90), 11, True)
    glove = material('fingerless_glove', 0x34403e, 11)
    iris = material('character_iris', 0x526358)

    def mesh(verts, faces, mat, smooth=True):
        data = bpy.data.meshes.new('sculpted_character_surface')
        data.from_pydata(verts, [], faces)
        data.update()
        bm = bmesh.new()
        bm.from_mesh(data)
        bmesh.ops.recalc_face_normals(bm, faces=list(bm.faces))
        bm.to_mesh(data)
        bm.free()
        obj = bpy.data.objects.new('sculpted_character_surface', data)
        bpy.context.collection.objects.link(obj)
        return finish(obj, mat, smooth=smooth)

    def form(rows, mat, sides=24, sculpt=None):
        """Asymmetric cross-sections: y, width, front depth, back depth, x, z.

        Continuous surfaces replace stacked ellipsoids at shoulders and joints.
        """
        verts, faces = [], []
        for row in rows:
            y, width, front, back, cx, cz = row
            for i in range(sides):
                a = math.tau * i / sides
                x = width * math.cos(a) + cx
                z = (front if math.sin(a) < 0 else back) * math.sin(a) + cz
                if sculpt:
                    x, y_out, z = sculpt(x, y, z, a)
                else:
                    y_out = y
                verts.append((x, y_out, z))
        for k in range(len(rows) - 1):
            for i in range(sides):
                n = (i + 1) % sides
                faces.append((k*sides+i, k*sides+n, (k+1)*sides+n, (k+1)*sides+i))
        faces.append(tuple(reversed(range(sides))))
        faces.append(tuple((len(rows)-1)*sides+i for i in range(sides)))
        return mesh(verts, faces, mat)

    def tube(points, radii, mat, sides=10, flattened=1.0):
        """Tapered swept volume for folds, fingers and broad sculpted hair locks."""
        verts, faces = [], []
        for j, point in enumerate(points):
            center = Vector(point)
            tangent = Vector(points[min(j+1, len(points)-1)]) - Vector(points[max(j-1, 0)])
            tangent.normalize()
            guide = Vector((1, 0, 0)) if abs(tangent.x) < .8 else Vector((0, 0, 1))
            u = tangent.cross(guide).normalized()
            v = tangent.cross(u).normalized()
            for i in range(sides):
                a = math.tau*i/sides
                verts.append(tuple(center + u*(math.cos(a)*radii[j])
                                   + v*(math.sin(a)*radii[j]*flattened)))
        for j in range(len(points)-1):
            for i in range(sides):
                n = (i+1) % sides
                faces.append((j*sides+i, j*sides+n, (j+1)*sides+n, (j+1)*sides+i))
        faces.append(tuple(reversed(range(sides))))
        faces.append(tuple((len(points)-1)*sides+i for i in range(sides)))
        return mesh(verts, faces, mat)

    def patch(points, thickness, mat, smooth=False):
        """Bevel-like convex apparel/face panel, with a raised central surface."""
        count = len(points)
        verts = [tuple(p) for p in points]
        verts += [(x, y, z+thickness) for x, y, z in points]
        center = tuple(sum(p[k] for p in points)/count for k in range(3))
        verts.append((center[0], center[1], center[2]-thickness*.22))
        faces = []
        for i in range(count):
            n = (i+1) % count
            faces.append((i, n, count*2))
            faces.append((i, count+i, count+n, n))
        faces.append(tuple(count+i for i in reversed(range(count))))
        return mesh(verts, faces, mat, smooth)

    begin('CharTorso')
    form([
        (-.008,.159,.106,.106,0,0), (.034,.168,.110,.108,0,0),
        (.120,.173,.116,.113,0,0), (.245,.193,.131,.119,0,0),
        (.355,.222,.145,.125,0,0), (.414,.236,.137,.127,0,0),
        (.457,.219,.121,.118,0,0), (.493,.157,.098,.097,0,0),
        (.512,.108,.078,.077,0,0),
    ], cloth)
    # The inset zipper and tailored pocket flaps read from a third-person camera.
    tube([(0,.020,-.111),(0,.17,-.126),(0,.33,-.147),(0,.458,-.128)],
         [.0045,.0045,.0045,.004], dark, 8)
    box((0,.383,-.151),(.015,.034,.008), steel, .003)
    for s in (-1, 1):
        patch([(s*.044,.289,-.134),(s*.155,.302,-.122),
               (s*.163,.227,-.124),(s*.049,.215,-.132)], .012, cloth)
        tube([(s*.049,.281,-.142),(s*.100,.286,-.143),(s*.154,.294,-.131)],
             [.0035,.0035,.0035], seam, 6)
        tube([(s*.146,.072,-.067),(s*.164,.124,-.079),(s*.180,.191,-.081)],
             [.003,.004,.002], seam, 6)
        tube([(s*.13,.030,-.097),(s*.151,.05,-.102),(s*.162,.072,-.09)],
             [.002,.004,.0015], cloth_hi, 6)
        # A chest harness continues onto the backpack; it is a flat woven band.
        patch([(s*.156,.461,-.100),(s*.176,.454,-.097),
               (s*.133,.117,-.114),(s*.115,.12,-.116)], .008, strap)
        box((s*.145,.276,-.144),(.028,.034,.010), steel, .004)

    begin('CharTrim')
    # A shaped shoulder yoke, two small epaulettes and a standing collar.
    for s in (-1, 1):
        patch([(s*.017,.431,-.137),(s*.129,.491,-.109),
               (s*.220,.457,-.111),(s*.231,.397,-.137),
               (s*.084,.402,-.153)], .010, accent)
        patch([(s*.159,.470,.073),(s*.226,.434,.088),
               (s*.220,.401,.121),(s*.14,.426,.127)], -.009, accent)
        box((s*.209,.449,-.006),(.072,.038,.154), accent, .012)
    form([(.491,.100,.077,.078,0,0),(.517,.079,.068,.070,0,0),
          (.543,.073,.067,.069,0,0)], strap, 20)
    for s in (-1, 1):
        patch([(s*.01,.51,-.073),(s*.06,.533,-.068),
               (s*.093,.494,-.086),(s*.069,.454,-.128)], .005, accent)

    begin('CharPelvis')
    form([(-.125,.164,.103,.104,0,0),(-.075,.181,.114,.118,0,0),
          (-.019,.177,.110,.112,0,0),(.026,.168,.108,.109,0,0),
          (.058,.163,.106,.106,0,0)], pants)
    form([(.021,.176,.117,.118,0,0),(.055,.171,.115,.114,0,0)], dark)
    box((0,.039,-.121),(.043,.033,.013), steel, .005)
    for s in (-1, 1):
        box((s*.114,.039,-.090),(.023,.043,.018), strap, .004)
        tube([(s*.071,-.09,-.093),(s*.121,-.048,-.095),(s*.158,-.019,-.069)],
             [.003,.003,.002], pant_shadow, 6)
        box((s*.178,-.045,.023),(.032,.083,.075), strap, .008)

    begin('CharHead')
    form([(-.045,.042,.043,.046,0,.005),(.001,.048,.050,.052,0,.008),
          (.051,.047,.050,.048,0,.010)], skin, 20)

    def face_sculpt(x, y, z, angle):
        front = max(0.0, -math.sin(angle))
        # Flatten the facial plane, inset the orbital area, broaden cheekbones,
        # and keep a clean tapered jaw instead of intersecting face spheres.
        if z < 0:
            z += .009*front*math.exp(-((y-.157)/.029)**2)
            z -= .009*front*math.exp(-((abs(x)-.069)/.026)**2)*math.exp(-((y-.113)/.039)**2)
            z -= .005*front*math.exp(-((y-.050)/.022)**2)
        return x, y, z

    form([
        (.006,.047,.044,.055,0,.010),(.024,.063,.072,.065,0,.005),
        (.043,.080,.088,.077,0,.004),(.073,.099,.096,.088,0,.003),
        (.107,.111,.102,.098,0,.004),(.138,.115,.106,.106,0,.005),
        (.169,.112,.106,.110,0,.006),(.203,.109,.099,.105,0,.010),
        (.232,.096,.084,.093,0,.012),(.251,.074,.063,.070,0,.013),
        (.264,.037,.030,.035,0,.013),(.268,.010,.009,.010,0,.013),
    ], skin, 28, face_sculpt)
    # Angular bridge, rounded nose wings and shallow nostrils are a single
    # small sculptural assembly, rather than a sphere stuck to the face.
    mesh([(-.014,.182,-.102),(.014,.182,-.102),(-.013,.124,-.128),
          (.013,.124,-.128),(-.026,.099,-.116),(.026,.099,-.116),
          (-.012,.098,-.142),(.012,.098,-.142),(0,.089,-.124)],
         [(0,1,3,2),(2,3,7,6),(0,2,4),(1,5,3),(4,2,6,8),
          (3,5,8,7),(6,7,8),(4,8,5),(0,4,5,1)], skin)
    for s in (-1, 1):
        tube([(s*.018,.098,-.120),(s*.020,.093,-.119),(s*.013,.090,-.123)],
             [.003,.0035,.002], skin_shadow, 8)
        # An ear shell has a visibly inset inner fold and integrated lobe.
        form([(.066,.008,.009,.009,s*.111,.012),
              (.084,.018,.015,.015,s*.115,.009),
              (.115,.020,.017,.018,s*.116,.007),
              (.136,.015,.015,.014,s*.113,.008),
              (.145,.006,.008,.009,s*.109,.010)], skin, 12)
        tube([(s*.119,.082,-.004),(s*.123,.112,-.008),(s*.117,.130,-.003)],
             [.005,.006,.004], skin_shadow, 8)
        # Eyes sit inside continuous brow/cheek surfaces with upper/lower lids.
        sphere((s*.042,.158,-.088),(.029,.014,.009), white, 16, 8)
        sphere((s*.041,.157,-.096),(.0105,.0105,.004), iris, 12, 8)
        sphere((s*.041,.157,-.100),(.0055,.0065,.002), black, 10, 6)
        sphere((s*.038,.160,-.102),(.002,.002,.001), white, 8, 6)
        tube([(s*.013,.163,-.092),(s*.042,.171,-.091),(s*.071,.162,-.083)],
             [.003,.0045,.002], skin_shadow, 8)
        tube([(s*.016,.149,-.091),(s*.043,.145,-.091),(s*.068,.151,-.084)],
             [.002,.0025,.0015], skin, 8)
        tube([(s*.015,.186,-.099),(s*.039,.190,-.102),(s*.070,.181,-.092)],
             [.004,.0055,.002], dark, 8, 1.2)
    tube([(-.031,.059,-.090),(-.014,.064,-.096),(0,.062,-.098),
          (.014,.064,-.096),(.031,.059,-.090)],
         [.0015,.003,.0025,.003,.0015], lip, 8)
    tube([(-.026,.057,-.094),(0,.054,-.098),(.026,.057,-.094)],
         [.001,.0018,.001], skin_shadow, 8)

    def scalp(mat, swept=False):
        verts, faces = [], []
        sides, rings = 24, 9
        # Expanded cranium sections keep the shell outside the head at every
        # height, including the occiput. A simple radial dome either clips the
        # crown or leaves a large shaved patch above the nape.
        profile = [
            (.055,.103,.107,.100,.004),(.090,.118,.112,.111,.004),
            (.130,.121,.114,.119,.005),(.170,.120,.114,.120,.006),
            (.205,.116,.107,.115,.010),(.238,.104,.094,.103,.012),
            (.264,.072,.062,.067,.013),(.283,.041,.034,.037,.013),
            (.294,.008,.007,.008,.013),
        ]

        def section(y):
            for first, second in zip(profile, profile[1:]):
                if y <= second[0]:
                    t = min(1, max(0, (y-first[0])/(second[0]-first[0])))
                    return tuple(first[k]+(second[k]-first[k])*t for k in range(1,5))
            return profile[-1][1:]

        for j in range(rings):
            t = j/(rings-1)
            for i in range(sides):
                a = math.tau*i/sides
                # Forehead .212m, temple .165m, nape .075m: a clean short cut
                # follows the ears instead of stopping across the rear skull.
                edge = .1435-.0685*math.sin(a)+.0215*math.cos(a)**2
                y = edge+(.294-edge)*t
                if swept:
                    y += .009*max(0,-math.cos(a))*math.sin(math.pi*t)
                rx, front, back, z = section(y)
                verts.append((rx*math.cos(a), y,
                              z+(front if math.sin(a)<0 else back)*math.sin(a)))
        for j in range(rings-1):
            for i in range(sides):
                n = (i+1) % sides
                faces.append((j*sides+i,j*sides+n,(j+1)*sides+n,(j+1)*sides+i))
        faces.append(tuple((rings-1)*sides+i for i in range(sides)))
        mesh(verts, faces, mat)

    begin('Hair1')
    scalp(hair, True)
    # Broad swept quiff ridges merge with a continuous scalp; no bead-like tufts.
    for i in range(6):
        x = -.077+i*.028
        lift = .015*(1-abs(x)/.10)
        tube([(x,.236,.082),(x-.012,.277,.035),(x+.016,.284+lift,-.039),
              (x+.030,.252+lift,-.082)], [.022,.025,.023,.004], hair, 10, .72)
    for s in (-1, 1):
        tube([(s*.104,.205,.043),(s*.111,.167,.025),(s*.105,.149,.009)],
             [.016,.014,.005], hair_shadow, 10, .65)
    tube([(-.074,.28,.015),(-.031,.301,-.030),(.015,.283,-.070)],
         [.003,.004,.001], hair_hi, 8)

    begin('Hair2')
    scalp(hair)
    # A loose bob silhouette with parted fringe and tapering side locks.
    for s in (-1, 1):
        for i in range(3):
            z = -.012+i*.039
            tube([(s*.089,.239,z),(s*.111,.169,z+.019),
                  (s*.116,.096,z+.029),(s*.093,.057,z+.025)],
                 [.026,.026,.024,.007], hair, 10, .86)
        tube([(s*.015,.274,-.035),(s*.054,.255,-.080),
              (s*.094,.208,-.072),(s*.105,.171,-.024)],
             [.025,.027,.021,.004], hair, 10, .72)
        tube([(s*.101,.199,.036),(s*.126,.130,.059),(s*.103,.077,.079)],
             [.003,.004,.001], hair_hi, 8)
    for x in (-.057,0,.057):
        tube([(x,.231,.084),(x,.151,.119),(x,.070,.111)],
             [.030,.034,.006], hair_shadow, 10)

    begin('Hair3')
    scalp(hair)
    for s in (-1, 1):
        tube([(s*.034,.269,-.057),(s*.096,.232,-.049),
              (s*.098,.179,.061),(s*.044,.184,.118)],
             [.025,.024,.022,.013], hair, 10, .8)
    cylinder((0,.185,.108),(0,.180,.147),.031,.028,strap,16)
    tube([(0,.188,.142),(.008,.150,.169),(.017,.102,.169),
          (.034,.055,.145),(.043,.013,.154)],
         [.026,.035,.030,.023,.003], hair, 12, .90)
    tube([(.016,.17,.165),(.038,.105,.174),(.046,.031,.157)],
         [.003,.004,.001], hair_hi, 8)

    begin('CharArmUp')
    form([(.030,.023,.023,.023,0,0),(.021,.054,.055,.054,0,0),
          (.004,.073,.075,.074,0,0),(-.035,.081,.080,.076,0,0),
          (-.091,.076,.073,.071,0,0),(-.175,.067,.065,.063,0,0),
          (-.243,.055,.056,.055,0,.002),(-.289,.054,.055,.053,0,.002)], cloth, 20)
    for s in (-1, 1):
        tube([(s*.061,-.037,-.037),(s*.069,-.068,-.041),(s*.059,-.108,-.037)],
             [.002,.004,.002], cloth_hi, 6)
    patch([(-.042,-.204,.049),(.042,-.204,.049),(.045,-.276,.047),
           (-.044,-.277,.047)], -.016, strap)
    box((0,-.094,-.075),(.071,.067,.008), accent, .009)

    begin('CharArmLow')
    form([(0,.054,.055,.055,0,0),(-.035,.060,.060,.056,0,0),
          (-.090,.064,.059,.056,0,0),(-.155,.055,.053,.049,0,0),
          (-.228,.044,.044,.042,0,0),(-.271,.040,.041,.040,0,0)], cloth, 20)
    patch([(-.039,-.061,-.054),(.039,-.061,-.054),(.029,-.181,-.049),
           (-.029,-.181,-.049)], .013, seam)
    for y in (-.080,-.111,-.142):
        tube([(-.027,y,-.068),(0,y-.007,-.070),(.027,y,-.068)],
             [.0025,.003,.0025], cloth_hi, 6)
    form([(-.233,.047,.046,.045,0,0),(-.268,.045,.044,.043,0,0)], strap, 20)
    box((0,-.250,.047),(.042,.028,.009), steel, .005)

    begin('CharHand')
    form([(.003,.038,.026,.027,0,0),(-.020,.041,.028,.028,0,-.001),
          (-.051,.039,.026,.025,0,-.006),(-.068,.032,.022,.021,0,-.009)], glove, 16)
    patch([(-.023,-.018,.028),(.023,-.018,.028),(.027,-.050,.020),
           (-.025,-.054,.020)], -.008, dark)
    for i in range(4):
        x = -.025+i*.016
        end = -.094 + (.008 if i in (0,3) else 0)
        tube([(x,-.061,-.010),(x,-.078,-.013),(x,end,-.022)],
             [.0085,.008,.005], skin, 10, .88)
        tube([(x,-.054,-.011),(x,-.067,-.013)], [.010,.009], glove, 8)
    tube([(.030,-.017,-.015),(.044,-.034,-.020),(.042,-.053,-.026)],
         [.015,.012,.007], skin, 12, .85)
    tube([(.030,-.014,-.012),(.039,-.029,-.019)], [.017,.013], glove, 10)

    begin('CharLegUp')
    form([(0,.089,.094,.099,0,0),(-.050,.098,.099,.105,0,0),
          (-.120,.101,.105,.106,0,0),(-.222,.087,.092,.089,0,.001),
          (-.335,.071,.075,.072,0,.002),(-.430,.064,.068,.066,0,.002)], pants, 22)
    for s in (-1, 1):
        box((s*.094,-.188,.011),(.027,.129,.095), pants, .010)
        box((s*.096,-.141,.011),(.029,.027,.102), pant_shadow, .008)
        tube([(s*.074,-.282,-.028),(s*.066,-.309,-.045),(s*.052,-.329,-.060)],
             [.003,.004,.0015], pant_shadow, 6)
    patch([(-.046,-.366,-.064),(.046,-.366,-.064),(.048,-.418,-.066),
           (-.048,-.418,-.066)], .010, strap)

    begin('CharLegLow')
    form([(0,.064,.067,.068,0,0),(-.067,.071,.074,.077,0,.002),
          (-.151,.067,.069,.072,0,.003),(-.235,.057,.062,.062,0,.003),
          (-.337,.049,.053,.052,0,.002),(-.400,.047,.050,.049,0,0)], pants, 22)
    # A broad chamfered knee guard with a raised plate and two strap anchors.
    patch([(-.041,.009,-.067),(.041,.009,-.067),(.053,-.013,-.075),
           (.045,-.090,-.077),(0,-.112,-.075),(-.045,-.090,-.077),
           (-.053,-.013,-.075)], .019, strap)
    patch([(-.031,-.017,-.092),(.031,-.017,-.092),(.034,-.071,-.098),
           (0,-.083,-.099),(-.034,-.071,-.098)], .008, dark)
    for s in (-1, 1):
        box((s*.062,-.037,.009),(.014,.036,.059), strap, .004)
    tube([(-.033,-.193,-.051),(-.013,-.211,-.061),(.028,-.217,-.050)],
         [.002,.0035,.0015], pant_shadow, 6)
    form([(-.351,.052,.056,.055,0,0),(-.397,.051,.054,.053,0,0)], strap, 18)

    begin('CharBoot')
    form([(.057,.051,.055,.057,0,0),(.024,.058,.064,.064,0,-.004),
          (-.008,.064,.105,.064,0,-.012),(-.041,.068,.155,.069,0,-.014),
          (-.065,.068,.159,.070,0,-.014)], boot_mat, 24)
    form([(-.060,.072,.164,.074,0,-.014),(-.083,.073,.165,.074,0,-.014)], dark, 24)
    patch([(-.054,-.019,-.120),(.054,-.019,-.120),(.055,-.046,-.167),
           (-.055,-.046,-.167)], .009, strap)
    patch([(-.036,.028,-.062),(.036,.028,-.062),(.042,-.023,-.108),
           (-.042,-.023,-.108)], .008, dark)
    for y, z in [(.024,-.071),(.008,-.085),(-.008,-.107)]:
        tube([(-.034,y,z),(0,y-.006,z-.004),(.034,y,z)], [.003,.003,.003], white, 8)
    box((0,.015,.066),(.057,.053,.014), strap, .006)
    for s in (-1, 1):
        tube([(s*.052,.023,.015),(s*.065,-.007,.016),(s*.068,-.052,.008)],
             [.0025,.004,.002], seam, 6)

    begin('CharBackpack')
    # Padded hiking-pack volume, not a thin floating cube; straps stay inside
    # the established .93m torso pivot and the original front/back envelope.
    box((0,.291,.174),(.308,.346,.144), cloth, .039)
    box((0,.243,.253),(.244,.214,.060), cloth, .024)
    box((0,.379,.248),(.189,.101,.026), dark, .011)
    patch([(-.101,.321,.278),(.101,.321,.278),(.104,.190,.280),
           (-.104,.190,.280)], -.010, seam)
    tube([(-.092,.309,.287),(0,.314,.292),(.092,.309,.287)], [.004,.004,.004], strap, 8)
    box((.075,.287,.294),(.016,.034,.011), steel, .003)
    for s in (-1, 1):
        box((s*.146,.229,.168),(.047,.134,.106), strap, .012)
        tube([(s*.117,.459,.117),(s*.126,.355,.121),(s*.122,.110,.119)],
             [.014,.014,.011], strap, 10, .55)
        box((s*.119,.157,.277),(.026,.043,.013), steel, .004)
    tube([(-.053,.459,.172),(-.041,.477,.172),(.041,.477,.172),(.053,.459,.172)],
         [.011,.011,.011,.011], strap, 10)
