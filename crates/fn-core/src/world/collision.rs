//! Collision shapes and the spatial hash used for movement, bullets, camera and AI.
//!
//! The world is made of a heightfield terrain plus three solid shapes:
//! axis-aligned boxes (buildings, floors, walls), vertical cylinders (trunks,
//! rocks) and wedges (ramps, stairs, roofs). Shapes are *solid below their top
//! surface*, which gives a single rule for movement: a shape whose top is within
//! step height of the feet is walkable, otherwise it blocks.

use crate::math::*;

/// What a collider belongs to, so hits can be attributed (trees can be
/// harvested, building pieces damaged ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Static,
    Tree(u32),
    Rock(u32),
    Piece(u32),
    Building(u32),
    Prop(u32),
}

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Box { min: Vec3, max: Vec3 },
    /// Vertical cylinder.
    Cyl { cx: f32, cz: f32, r: f32, y0: f32, y1: f32 },
    /// Solid wedge whose slope rises toward `dir` (0 +X, 1 +Z, 2 -X, 3 -Z).
    Wedge { min: Vec3, max: Vec3, dir: u8 },
}

#[derive(Clone, Copy, Debug)]
pub struct Collider {
    pub shape: Shape,
    pub tag: Tag,
    /// Bullets pass through when false (e.g. low decorative props).
    pub blocks_bullets: bool,
}

impl Collider {
    pub fn new(shape: Shape, tag: Tag) -> Self {
        Self { shape, tag, blocks_bullets: true }
    }
    pub fn aabb_box(min: Vec3, max: Vec3, tag: Tag) -> Self {
        Self::new(Shape::Box { min, max }, tag)
    }
    pub fn cyl(cx: f32, cz: f32, r: f32, y0: f32, y1: f32, tag: Tag) -> Self {
        Self::new(Shape::Cyl { cx, cz, r, y0, y1 }, tag)
    }
    pub fn wedge(min: Vec3, max: Vec3, dir: u8, tag: Tag) -> Self {
        Self::new(Shape::Wedge { min, max, dir }, tag)
    }
}

impl Shape {
    pub fn aabb(&self) -> Aabb {
        match *self {
            Shape::Box { min, max } | Shape::Wedge { min, max, .. } => Aabb::new(min, max),
            Shape::Cyl { cx, cz, r, y0, y1 } => Aabb::new(Vec3::new(cx - r, y0, cz - r), Vec3::new(cx + r, y1, cz + r)),
        }
    }

    /// Height of the top surface at (x, z), if the point is inside the footprint.
    pub fn top_at(&self, x: f32, z: f32) -> Option<f32> {
        match *self {
            Shape::Box { min, max } => {
                if x >= min.x && x <= max.x && z >= min.z && z <= max.z {
                    Some(max.y)
                } else {
                    None
                }
            }
            Shape::Cyl { cx, cz, r, y1, .. } => {
                let (dx, dz) = (x - cx, z - cz);
                if dx * dx + dz * dz <= r * r {
                    Some(y1)
                } else {
                    None
                }
            }
            Shape::Wedge { min, max, dir } => {
                if x < min.x || x > max.x || z < min.z || z > max.z {
                    return None;
                }
                Some(wedge_surface(min, max, dir, x, z))
            }
        }
    }

    /// Height of the underside at (x, z) (used to let actors walk under bridges).
    pub fn bottom(&self) -> f32 {
        match *self {
            Shape::Box { min, .. } | Shape::Wedge { min, .. } => min.y,
            Shape::Cyl { y0, .. } => y0,
        }
    }

    /// If a circle (radius `r`) centred at `c` overlaps the footprint of this
    /// shape and the shape is not walkable from `feet` (with `step` allowance)
    /// nor above `head`, return the displacement that pushes it out.
    pub fn push_out(&self, c: Vec2, r: f32, feet: f32, head: f32, step: f32) -> Option<Vec2> {
        match *self {
            Shape::Box { min, max } => {
                if max.y <= feet + step || min.y >= head {
                    return None;
                }
                push_out_rect(c, r, Vec2::new(min.x, min.z), Vec2::new(max.x, max.z))
            }
            Shape::Cyl { cx, cz, r: cr, y0, y1 } => {
                if y1 <= feet + step || y0 >= head {
                    return None;
                }
                let d = c - Vec2::new(cx, cz);
                let l = d.length();
                let rr = r + cr;
                if l >= rr {
                    return None;
                }
                if l < 1e-5 {
                    return Some(Vec2::new(rr, 0.0));
                }
                Some(d / l * (rr - l))
            }
            Shape::Wedge { min, max, dir } => {
                if min.y >= head {
                    return None;
                }
                // Use the surface height at the closest footprint point.
                let cp = Vec2::new(c.x.clamp(min.x, max.x), c.y.clamp(min.z, max.z));
                let ht = wedge_surface(min, max, dir, cp.x, cp.y);
                if ht <= feet + step {
                    return None;
                }
                push_out_rect(c, r, Vec2::new(min.x, min.z), Vec2::new(max.x, max.z))
            }
        }
    }

    /// Ray intersection; returns (t, outward normal).
    pub fn raycast(&self, o: Vec3, d: Vec3, max_t: f32) -> Option<(f32, Vec3)> {
        match *self {
            Shape::Box { min, max } => {
                let b = Aabb::new(min, max);
                let inv = Vec3::new(safe_inv(d.x), safe_inv(d.y), safe_inv(d.z));
                let (near, _far) = b.ray(o, inv, max_t)?;
                if b.contains(o) {
                    return None; // started inside; ignore (camera / muzzle inside geometry)
                }
                let p = o + d * near;
                Some((near, b.hit_normal(p)))
            }
            Shape::Cyl { cx, cz, r, y0, y1 } => ray_cyl(o, d, max_t, cx, cz, r, y0, y1),
            Shape::Wedge { min, max, dir } => ray_wedge(o, d, max_t, min, max, dir),
        }
    }
}

#[inline]
fn safe_inv(v: f32) -> f32 {
    if v.abs() < 1e-9 {
        1e9f32.copysign(if v == 0.0 { 1.0 } else { v })
    } else {
        1.0 / v
    }
}

/// Surface height of a wedge at (x, z) (clamped to the footprint).
pub fn wedge_surface(min: Vec3, max: Vec3, dir: u8, x: f32, z: f32) -> f32 {
    let t = match dir & 3 {
        0 => (x - min.x) / (max.x - min.x).max(1e-5),
        1 => (z - min.z) / (max.z - min.z).max(1e-5),
        2 => (max.x - x) / (max.x - min.x).max(1e-5),
        _ => (max.z - z) / (max.z - min.z).max(1e-5),
    };
    min.y + (max.y - min.y) * t.clamp(0.0, 1.0)
}

fn push_out_rect(c: Vec2, r: f32, rmin: Vec2, rmax: Vec2) -> Option<Vec2> {
    let cp = Vec2::new(c.x.clamp(rmin.x, rmax.x), c.y.clamp(rmin.y, rmax.y));
    let d = c - cp;
    let l2 = d.length_squared();
    if l2 > 1e-10 {
        let l = l2.sqrt();
        if l >= r {
            return None;
        }
        return Some(d / l * (r - l));
    }
    // Centre is inside the rect: push out along the shortest axis.
    let left = c.x - rmin.x;
    let right = rmax.x - c.x;
    let down = c.y - rmin.y;
    let up = rmax.y - c.y;
    let m = left.min(right).min(down).min(up);
    Some(if m == left {
        Vec2::new(-(left + r), 0.0)
    } else if m == right {
        Vec2::new(right + r, 0.0)
    } else if m == down {
        Vec2::new(0.0, -(down + r))
    } else {
        Vec2::new(0.0, up + r)
    })
}

fn ray_cyl(o: Vec3, d: Vec3, max_t: f32, cx: f32, cz: f32, r: f32, y0: f32, y1: f32) -> Option<(f32, Vec3)> {
    let mut best: Option<(f32, Vec3)> = None;
    let mut cons = |t: f32, n: Vec3| {
        if t >= 0.0 && t <= max_t && best.map_or(true, |(b, _)| t < b) {
            best = Some((t, n));
        }
    };
    let ox = o.x - cx;
    let oz = o.z - cz;
    let a = d.x * d.x + d.z * d.z;
    if a > 1e-9 {
        let b = ox * d.x + oz * d.z;
        let c = ox * ox + oz * oz - r * r;
        let disc = b * b - a * c;
        if disc >= 0.0 {
            let s = disc.sqrt();
            let t = (-b - s) / a;
            let y = o.y + d.y * t;
            if t >= 0.0 && y >= y0 && y <= y1 {
                let p = o + d * t;
                cons(t, Vec3::new(p.x - cx, 0.0, p.z - cz).normalize_or_zero());
            }
        }
    }
    if d.y.abs() > 1e-9 {
        for (yy, n) in [(y1, Vec3::Y), (y0, Vec3::NEG_Y)] {
            let t = (yy - o.y) / d.y;
            if t >= 0.0 {
                let p = o + d * t;
                let (dx, dz) = (p.x - cx, p.z - cz);
                if dx * dx + dz * dz <= r * r && (o.y - yy) * n.y >= 0.0 {
                    cons(t, n);
                }
            }
        }
    }
    best
}

fn ray_wedge(o: Vec3, d: Vec3, max_t: f32, min: Vec3, max: Vec3, dir: u8) -> Option<(f32, Vec3)> {
    // Convex polytope: intersect half-spaces.
    let (hx, hy) = match dir & 3 {
        0 | 2 => (max.x - min.x, max.y - min.y),
        _ => (max.z - min.z, max.y - min.y),
    };
    let sn = match dir & 3 {
        0 => Vec3::new(-hy, hx, 0.0),
        1 => Vec3::new(0.0, hx, -hy),
        2 => Vec3::new(hy, hx, 0.0),
        _ => Vec3::new(0.0, hx, hy),
    }
    .normalize();
    // slope plane passes through the low-edge point.
    let low_pt = match dir & 3 {
        0 => Vec3::new(min.x, min.y, min.z),
        1 => Vec3::new(min.x, min.y, min.z),
        2 => Vec3::new(max.x, min.y, min.z),
        _ => Vec3::new(min.x, min.y, max.z),
    };
    let planes: [(Vec3, f32); 5] = [
        (Vec3::NEG_Y, -min.y),
        (Vec3::X, max.x),
        (Vec3::NEG_X, -min.x),
        (Vec3::Z, max.z),
        (Vec3::NEG_Z, -min.z),
    ];
    // Side/back walls only apply for the relevant faces: x/z bounds are all
    // planes of the footprint prism; the slope plane trims the top.
    let mut t_enter = 0.0f32;
    let mut t_exit = max_t;
    let mut enter_n = Vec3::ZERO;
    let check = |n: Vec3, dist: f32, o: Vec3, d: Vec3, t_enter: &mut f32, t_exit: &mut f32, enter_n: &mut Vec3| -> bool {
        let denom = n.dot(d);
        let num = dist - n.dot(o);
        if denom.abs() < 1e-9 {
            return num >= 0.0; // parallel: inside or outside the slab
        }
        let t = num / denom;
        if denom < 0.0 {
            if t > *t_enter {
                *t_enter = t;
                *enter_n = n;
            }
        } else if t < *t_exit {
            *t_exit = t;
        }
        *t_enter <= *t_exit
    };
    for (n, dist) in planes {
        if !check(n, dist, o, d, &mut t_enter, &mut t_exit, &mut enter_n) {
            return None;
        }
    }
    if !check(sn, sn.dot(low_pt), o, d, &mut t_enter, &mut t_exit, &mut enter_n) {
        return None;
    }
    if t_enter > 0.0 && t_enter <= t_exit && t_enter <= max_t && enter_n != Vec3::ZERO {
        Some((t_enter, enter_n))
    } else {
        None
    }
}

/// Uniform grid broadphase over the XZ plane. Colliders are stored in a slab so
/// building pieces can be added and removed at runtime.
#[derive(Clone)]
pub struct SpatialGrid {
    #[allow(dead_code)]
    cell: f32,
    inv: f32,
    origin: f32,
    n: usize,
    cells: Vec<Vec<u32>>,
    pub items: Vec<Option<Collider>>,
    free: Vec<u32>,
}

impl SpatialGrid {
    pub fn new(half_extent: f32, cell: f32) -> Self {
        let n = ((half_extent * 2.0) / cell).ceil() as usize + 1;
        Self { cell, inv: 1.0 / cell, origin: -half_extent, n, cells: vec![Vec::new(); n * n], items: Vec::new(), free: Vec::new() }
    }

    #[inline]
    fn ci(&self, v: f32) -> usize {
        (((v - self.origin) * self.inv).floor().max(0.0) as usize).min(self.n - 1)
    }

    pub fn len(&self) -> usize {
        self.items.len() - self.free.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn insert(&mut self, c: Collider) -> u32 {
        let id = if let Some(id) = self.free.pop() {
            self.items[id as usize] = Some(c);
            id
        } else {
            self.items.push(Some(c));
            (self.items.len() - 1) as u32
        };
        let b = c.shape.aabb();
        let (x0, x1, z0, z1) = (self.ci(b.min.x), self.ci(b.max.x), self.ci(b.min.z), self.ci(b.max.z));
        for z in z0..=z1 {
            for x in x0..=x1 {
                self.cells[z * self.n + x].push(id);
            }
        }
        id
    }

    pub fn remove(&mut self, id: u32) {
        if let Some(c) = self.items.get_mut(id as usize).and_then(|c| c.take()) {
            let b = c.shape.aabb();
            let (x0, x1, z0, z1) = (self.ci(b.min.x), self.ci(b.max.x), self.ci(b.min.z), self.ci(b.max.z));
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let cell = &mut self.cells[z * self.n + x];
                    if let Some(p) = cell.iter().position(|&i| i == id) {
                        cell.swap_remove(p);
                    }
                }
            }
            self.free.push(id);
        }
    }

    pub fn get(&self, id: u32) -> Option<&Collider> {
        self.items.get(id as usize).and_then(|c| c.as_ref())
    }

    /// Visit every collider whose cell range overlaps `b`. Colliders spanning
    /// several cells may be visited more than once.
    pub fn query(&self, b: &Aabb, mut f: impl FnMut(u32, &Collider)) {
        let (x0, x1, z0, z1) = (self.ci(b.min.x), self.ci(b.max.x), self.ci(b.min.z), self.ci(b.max.z));
        for z in z0..=z1 {
            for x in x0..=x1 {
                for &id in &self.cells[z * self.n + x] {
                    if let Some(c) = &self.items[id as usize] {
                        f(id, c);
                    }
                }
            }
        }
    }

    /// Walk the grid cells traversed by a ray (2D DDA over XZ), calling `f` for
    /// each cell's colliders with the segment [t0, t1] inside the cell. Stops
    /// when `f` returns false.
    pub fn walk_ray(&self, o: Vec3, d: Vec3, max_t: f32, mut f: impl FnMut(&[u32], f32) -> bool) {
        let ox = (o.x - self.origin) * self.inv;
        let oz = (o.z - self.origin) * self.inv;
        let mut cx = ox.floor() as i32;
        let mut cz = oz.floor() as i32;
        let dx = d.x * self.inv;
        let dz = d.z * self.inv;
        let step_x = if dx > 0.0 { 1 } else { -1 };
        let step_z = if dz > 0.0 { 1 } else { -1 };
        let t_delta_x = if dx.abs() > 1e-9 { (1.0 / dx).abs() } else { f32::INFINITY };
        let t_delta_z = if dz.abs() > 1e-9 { (1.0 / dz).abs() } else { f32::INFINITY };
        let mut t_max_x = if dx.abs() > 1e-9 { ((if dx > 0.0 { cx as f32 + 1.0 } else { cx as f32 }) - ox) / dx } else { f32::INFINITY };
        let mut t_max_z = if dz.abs() > 1e-9 { ((if dz > 0.0 { cz as f32 + 1.0 } else { cz as f32 }) - oz) / dz } else { f32::INFINITY };
        let t_end = max_t; // parameter in world units == cell units because d is normalised and inv scales both
        let mut guard = 0;
        // Convert t (in cell units along the XZ projection) back to world distance:
        // dx,dz are scaled by inv so t_max_* are in units of the original t.
        loop {
            if cx >= 0 && cz >= 0 && (cx as usize) < self.n && (cz as usize) < self.n {
                let cell = &self.cells[cz as usize * self.n + cx as usize];
                let t_next = t_max_x.min(t_max_z);
                if !cell.is_empty() && !f(cell, t_next) {
                    return;
                }
            } else if guard > 4 && (cx < -2 || cz < -2 || cx > self.n as i32 + 2 || cz > self.n as i32 + 2) {
                return;
            }
            let t_next = t_max_x.min(t_max_z);
            if t_next > t_end {
                return;
            }
            if t_max_x < t_max_z {
                cx += step_x;
                t_max_x += t_delta_x;
            } else {
                cz += step_z;
                t_max_z += t_delta_z;
            }
            guard += 1;
            if guard > 4096 {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(min: [f32; 3], max: [f32; 3]) -> Shape {
        Shape::Box { min: Vec3::from(min), max: Vec3::from(max) }
    }

    #[test]
    fn box_top_and_push_out() {
        let s = bx([0.0, 0.0, 0.0], [2.0, 3.0, 2.0]);
        assert_eq!(s.top_at(1.0, 1.0), Some(3.0));
        assert_eq!(s.top_at(3.0, 1.0), None);
        // Actor approaching from -X, feet at ground: blocked
        let p = s.push_out(Vec2::new(-0.2, 1.0), 0.4, 0.0, 1.8, 0.5).unwrap();
        assert!(p.x < 0.0 && p.x > -0.25, "{p:?}");
        // Actor with feet near the top: walkable, no push
        assert!(s.push_out(Vec2::new(-0.2, 1.0), 0.4, 2.8, 4.6, 0.5).is_none());
        // Low step: walkable
        let step = bx([0.0, 0.0, 0.0], [2.0, 0.3, 2.0]);
        assert!(step.push_out(Vec2::new(-0.2, 1.0), 0.4, 0.0, 1.8, 0.5).is_none());
        // Overhead slab above head: ignored
        let slab = bx([0.0, 3.0, 0.0], [2.0, 3.3, 2.0]);
        assert!(slab.push_out(Vec2::new(-0.2, 1.0), 0.4, 0.0, 1.8, 0.5).is_none());
    }

    #[test]
    fn cylinder_push_out_is_radial() {
        let s = Shape::Cyl { cx: 0.0, cz: 0.0, r: 1.0, y0: 0.0, y1: 5.0 };
        let p = s.push_out(Vec2::new(1.1, 0.0), 0.4, 0.0, 1.8, 0.5).unwrap();
        assert!((p.x - 0.3).abs() < 1e-5 && p.y.abs() < 1e-5);
        assert!(s.push_out(Vec2::new(2.0, 0.0), 0.4, 0.0, 1.8, 0.5).is_none());
    }

    #[test]
    fn wedge_surface_and_walkability() {
        let min = Vec3::new(0.0, 0.0, 0.0);
        let max = Vec3::new(4.0, 3.0, 4.0);
        // rises toward +X
        assert!((wedge_surface(min, max, 0, 0.0, 2.0) - 0.0).abs() < 1e-5);
        assert!((wedge_surface(min, max, 0, 2.0, 2.0) - 1.5).abs() < 1e-5);
        assert!((wedge_surface(min, max, 0, 4.0, 2.0) - 3.0).abs() < 1e-5);
        assert!((wedge_surface(min, max, 2, 4.0, 2.0) - 0.0).abs() < 1e-5);
        assert!((wedge_surface(min, max, 1, 2.0, 4.0) - 3.0).abs() < 1e-5);
        assert!((wedge_surface(min, max, 3, 2.0, 0.0) - 3.0).abs() < 1e-5);
        let s = Shape::Wedge { min, max, dir: 0 };
        // Entering at the low edge from -X is walkable.
        assert!(s.push_out(Vec2::new(-0.2, 2.0), 0.4, 0.0, 1.8, 0.5).is_none());
        // Pressing into the high end (from +X side) at ground level is blocked.
        assert!(s.push_out(Vec2::new(4.2, 2.0), 0.4, 0.0, 1.8, 0.5).is_some());
        // Mid-slope from the side while at ground level: blocked (inside the solid).
        assert!(s.push_out(Vec2::new(2.0, -0.2), 0.4, 0.0, 1.8, 0.5).is_some());
        // Standing on the slope itself near the high end: walkable.
        assert!(s.push_out(Vec2::new(3.8, 2.0), 0.4, 2.7, 4.5, 0.5).is_none());
    }

    #[test]
    fn ray_hits_box_wedge_cyl() {
        let b = bx([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        let (t, n) = b.raycast(Vec3::new(-3.0, 1.0, 1.0), Vec3::X, 100.0).unwrap();
        assert!((t - 3.0).abs() < 1e-4 && n == Vec3::NEG_X);
        assert!(b.raycast(Vec3::new(-3.0, 5.0, 1.0), Vec3::X, 100.0).is_none());

        let c = Shape::Cyl { cx: 0.0, cz: 0.0, r: 1.0, y0: 0.0, y1: 4.0 };
        let (t, n) = c.raycast(Vec3::new(-5.0, 1.0, 0.0), Vec3::X, 100.0).unwrap();
        assert!((t - 4.0).abs() < 1e-4 && (n - Vec3::NEG_X).length() < 1e-4);
        let (t, n) = c.raycast(Vec3::new(0.0, 9.0, 0.0), Vec3::NEG_Y, 100.0).unwrap();
        assert!((t - 5.0).abs() < 1e-4 && n == Vec3::Y);
        assert!(c.raycast(Vec3::new(-5.0, 6.0, 0.0), Vec3::X, 100.0).is_none());

        let w = Shape::Wedge { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(4.0, 4.0, 4.0), dir: 0 };
        // From above hitting the slope mid-way: surface height at x=2 is 2.
        let (t, n) = w.raycast(Vec3::new(2.0, 10.0, 2.0), Vec3::NEG_Y, 100.0).unwrap();
        assert!((10.0 - t - 2.0).abs() < 1e-3, "t={t}");
        assert!(n.y > 0.0 && n.x < 0.0, "{n:?}");
        // Horizontally from +X side hits the back wall.
        let (t, n) = w.raycast(Vec3::new(10.0, 1.0, 2.0), Vec3::NEG_X, 100.0).unwrap();
        assert!((t - 6.0).abs() < 1e-4 && n == Vec3::X);
        // Horizontally from -X side at low height passes over the toe? y=1 is inside the wedge at x>=1.
        let (t, _) = w.raycast(Vec3::new(-5.0, 1.0, 2.0), Vec3::X, 100.0).unwrap();
        assert!((t - 6.0).abs() < 1e-3, "slope hit at x=1 -> t=6, got {t}");
    }

    #[test]
    fn grid_insert_query_remove() {
        let mut g = SpatialGrid::new(100.0, 8.0);
        let a = g.insert(Collider::aabb_box(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 3.0, 10.0), Tag::Static));
        let b = g.insert(Collider::cyl(-50.0, -50.0, 1.0, 0.0, 5.0, Tag::Tree(1)));
        assert_eq!(g.len(), 2);
        let mut seen = vec![];
        g.query(&Aabb::new(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 1.0)), |id, _| seen.push(id));
        assert!(seen.contains(&a) && !seen.contains(&b));
        g.remove(a);
        let mut seen = vec![];
        g.query(&Aabb::new(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 1.0)), |id, _| seen.push(id));
        assert!(seen.is_empty());
        let c = g.insert(Collider::aabb_box(Vec3::ZERO, Vec3::ONE, Tag::Static));
        assert_eq!(c, a, "slots are reused");
    }

    #[test]
    fn walk_ray_visits_cells_along_ray() {
        let mut g = SpatialGrid::new(100.0, 8.0);
        let far = g.insert(Collider::aabb_box(Vec3::new(40.0, 0.0, -1.0), Vec3::new(42.0, 3.0, 1.0), Tag::Static));
        let off = g.insert(Collider::aabb_box(Vec3::new(40.0, 0.0, 30.0), Vec3::new(42.0, 3.0, 32.0), Tag::Static));
        let mut hit = vec![];
        g.walk_ray(Vec3::new(0.0, 1.0, 0.0), Vec3::X, 100.0, |ids, _| {
            hit.extend_from_slice(ids);
            true
        });
        assert!(hit.contains(&far));
        assert!(!hit.contains(&off));
    }
}
