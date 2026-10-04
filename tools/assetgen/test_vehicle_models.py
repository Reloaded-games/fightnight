"""Check authored vehicle geometry in Blender without exporting repository assets.

Run: blender --background --factory-startup --python tools/assetgen/test_vehicle_models.py
This evaluates the same modifiers, transforms and packed vertices as the exporter.
"""
import ast
import math
from pathlib import Path
import struct
import sys

from mathutils.bvhtree import BVHTree

sys.path.insert(0, str(Path(__file__).resolve().parent))
import vehicle_models


def helper_api():
    path = Path(__file__).with_name('build_assets.py')
    tree = ast.parse(path.read_text(encoding='utf-8'), str(path))
    nodes = []
    for node in tree.body:
        if isinstance(node, ast.FunctionDef) and node.name == 'supplies':
            break
        if isinstance(node, ast.Import) and any(alias.name in {
            'character_models', 'character_animations', 'weapon_models', 'vehicle_models',
        } for alias in node.names):
            continue
        nodes.append(node)
    namespace = {'__file__': str(path), '__name__': 'vehicle_test_helpers'}
    exec(compile(ast.Module(body=nodes, type_ignores=[]), str(path), 'exec'), namespace)
    return namespace


def inspect_group(api, name):
    vertices, triangles = set(), 0
    low, high = [math.inf]*3, [-math.inf]*3
    deps = api['bpy'].context.evaluated_depsgraph_get()
    for obj in api['groups'][name]:
        evaluated = obj.evaluated_get(deps)
        mesh = evaluated.to_mesh()
        mesh.calc_loop_triangles()
        matrix = api['TO_ENGINE'] @ evaluated.matrix_world
        normal_matrix = matrix.to_3x3().inverted().transposed()
        mat = obj.data.materials[0]
        color = tuple(round(c*255) for c in mat['engine_rgb'])
        kind = int(mat['engine_mat'])
        for triangle in mesh.loop_triangles:
            a, b, c = (matrix @ mesh.vertices[i].co for i in triangle.vertices)
            if (b-a).cross(c-a).length_squared < 1e-14:
                continue
            triangles += 1
            for index, loop in zip(triangle.vertices, triangle.loops):
                p = matrix @ mesh.vertices[index].co
                n = (normal_matrix @ mesh.corner_normals[loop].vector).normalized()
                assert all(math.isfinite(x) for x in (*p, *n)), (name, 'non-finite vertex')
                assert abs(n.length-1) < .001, (name, 'invalid normal')
                for axis in range(3):
                    low[axis] = min(low[axis], p[axis])
                    high[axis] = max(high[axis], p[axis])
                sway = min(.85, max(0, p.y*.09)) if kind == 1 else min(1, max(0, p.y*1.6)) if kind == 2 else 0
                ao = .75+min(.25, max(0, p.y)*.035) if kind in (1, 2) else .94
                vertices.add(struct.pack('<6f8B', *p, *n, *color,
                    255 if mat['engine_tint'] else 0, round(ao*255), kind,
                    round(sway*255), round(mat['engine_spec']*255)))
        evaluated.to_mesh_clear()
    assert triangles > 100 and len(vertices) < 5000, (name, len(vertices), triangles)
    result = {'vertices': len(vertices), 'triangles': triangles,
              'bounds': [tuple(round(v, 3) for v in low), tuple(round(v, 3) for v in high)]}
    print('VEHICLE_CHECK', name, result, flush=True)
    return low, high


def check_driver_clearance(api, name):
    points, faces = [], []
    deps = api['bpy'].context.evaluated_depsgraph_get()
    for obj in api['groups'][name]:
        evaluated = obj.evaluated_get(deps)
        mesh = evaluated.to_mesh()
        mesh.calc_loop_triangles()
        matrix = api['TO_ENGINE'] @ evaluated.matrix_world
        start = len(points)
        points.extend(matrix @ vertex.co for vertex in mesh.vertices)
        faces.extend(tuple(start+i for i in triangle.vertices) for triangle in mesh.loop_triangles)
        evaluated.to_mesh_clear()
    bvh = BVHTree.FromPolygons(points, faces, all_triangles=True)
    # The pelvis bottom is .765m with the unchanged .89m hip pivot: it rests
    # above the cushion instead of passing through an over-high seat.
    cushion = bvh.ray_cast((-.38, .765, .30), (0, -1, 0), .5)[0]
    assert cushion is not None and .74 < cushion.y < .76, (name, 'seat intersects pelvis', cushion)
    floor = bvh.ray_cast((-.38, .43, -.30), (0, -1, 0), .5)[0]
    assert floor is not None and .35 < floor.y < .40, (name, 'no footwell clearance', floor)
    distance = bvh.find_nearest((-.38, 1.61, .30))[3]
    assert distance > .17, (name, 'driver head intersects cabin', distance)


def main():
    api = helper_api()
    groups = vehicle_models.build(api)
    assert set(groups) == set(vehicle_models.GROUPS) == set(api['groups'])
    body_bounds = [inspect_group(api, name) for name in ('VehicleBody', 'VehicleSport')]
    for name in ('VehicleBody', 'VehicleSport'):
        check_driver_clearance(api, name)
    wheel_low, wheel_high = inspect_group(api, 'VehicleWheel')
    # Both body shapes fit the same chassis and seated-player pivots.
    for low, high in body_bounds:
        assert 2.1 < high[0]-low[0] < 2.5
        assert 3.1 < high[2]-low[2] < 3.8
        assert .3 < low[1] < .5 and 1.4 < high[1] < 2.2
        assert low[0] < vehicle_models.DRIVER_HIP[0] < high[0]
    # A tyre rolls about X, is centred on its axle, and remains inside the .48m
    # collision/camera allowance including the raised tread silhouette.
    assert max(abs(wheel_low[0]), abs(wheel_high[0])) < .24
    for axis in (1, 2):
        assert abs(wheel_high[axis]+wheel_low[axis]) < .003
        assert .46 <= max(abs(wheel_low[axis]), abs(wheel_high[axis])) < .48
    assert vehicle_models.WHEEL_CENTERS == tuple((x, .47, z)
        for x in (-1.02, 1.02) for z in (-1.02, 1.05))
    print('VEHICLE_CHECK geometry budgets, transforms, shared pivots and driver clearances passed', flush=True)


if __name__ == '__main__':
    main()
