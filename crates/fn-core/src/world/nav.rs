//! Grid navigation: A* over a coarse cell grid with a pluggable cost function,
//! used for road planning at world-generation time and by bots at runtime.

use crate::math::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

#[derive(Clone)]
pub struct NavGrid {
    pub w: usize,
    pub h: usize,
    pub cell: f32,
    /// World position of the grid's min corner.
    pub origin: Vec2,
    /// 0 = free, otherwise blocked.
    pub blocked: Vec<u8>,
}

const SQRT2_X16: u32 = 23; // 1.414 * 16
const ONE_X16: u32 = 16;

impl NavGrid {
    pub fn new(world_half: f32, cell: f32) -> Self {
        let n = ((world_half * 2.0) / cell).ceil() as usize;
        Self { w: n, h: n, cell, origin: Vec2::splat(-world_half), blocked: vec![0; n * n] }
    }

    #[inline]
    pub fn cell_of(&self, p: Vec2) -> (i32, i32) {
        (((p.x - self.origin.x) / self.cell).floor() as i32, ((p.y - self.origin.y) / self.cell).floor() as i32)
    }
    #[inline]
    pub fn center(&self, i: i32, j: i32) -> Vec2 {
        self.origin + Vec2::new((i as f32 + 0.5) * self.cell, (j as f32 + 0.5) * self.cell)
    }
    #[inline]
    pub fn in_bounds(&self, i: i32, j: i32) -> bool {
        i >= 0 && j >= 0 && (i as usize) < self.w && (j as usize) < self.h
    }
    #[inline]
    pub fn is_blocked(&self, i: i32, j: i32) -> bool {
        !self.in_bounds(i, j) || self.blocked[j as usize * self.w + i as usize] != 0
    }
    pub fn is_blocked_at(&self, p: Vec2) -> bool {
        let (i, j) = self.cell_of(p);
        self.is_blocked(i, j)
    }
    pub fn set_blocked(&mut self, i: i32, j: i32, v: bool) {
        if self.in_bounds(i, j) {
            self.blocked[j as usize * self.w + i as usize] = v as u8;
        }
    }

    /// Nearest free cell to `p` within a small search radius.
    pub fn nearest_free(&self, p: Vec2, max_r: i32) -> Option<(i32, i32)> {
        let (ci, cj) = self.cell_of(p);
        if !self.is_blocked(ci, cj) {
            return Some((ci, cj));
        }
        for r in 1..=max_r {
            let mut best: Option<(f32, i32, i32)> = None;
            for dj in -r..=r {
                for di in -r..=r {
                    if di.abs().max(dj.abs()) != r {
                        continue;
                    }
                    let (i, j) = (ci + di, cj + dj);
                    if !self.is_blocked(i, j) {
                        let d = self.center(i, j).distance_squared(p);
                        if best.map_or(true, |b| d < b.0) {
                            best = Some((d, i, j));
                        }
                    }
                }
            }
            if let Some((_, i, j)) = best {
                return Some((i, j));
            }
        }
        None
    }

    /// Supercover-ish line test: true if every cell along the segment is free.
    pub fn line_clear(&self, a: Vec2, b: Vec2) -> bool {
        let d = b - a;
        let len = d.length();
        if len < 1e-4 {
            return !self.is_blocked_at(a);
        }
        let steps = (len / (self.cell * 0.5)).ceil() as i32;
        for s in 0..=steps {
            let p = a + d * (s as f32 / steps as f32);
            if self.is_blocked_at(p) {
                return false;
            }
        }
        true
    }

    /// A* from `from` to `to`. `extra(i, j)` returns an additional per-cell cost
    /// (in cell-lengths) or `None` if the cell is impassable, on top of the
    /// grid's own blocked flags. Returns waypoints at cell centres (the first
    /// is the start cell, the last the goal cell), unsmoothed.
    pub fn find_path_with(
        &self,
        from: Vec2,
        to: Vec2,
        max_expansions: usize,
        extra: impl Fn(i32, i32) -> Option<f32>,
    ) -> Option<Vec<Vec2>> {
        let start = self.nearest_free(from, 6)?;
        let goal = self.nearest_free(to, 6)?;
        if start == goal {
            return Some(vec![self.center(start.0, start.1)]);
        }
        let w = self.w as i32;
        let idx = |i: i32, j: i32| (j * w + i) as usize;
        let mut g: Vec<u32> = vec![u32::MAX; self.w * self.h];
        let mut came: Vec<u32> = vec![u32::MAX; self.w * self.h];
        let mut closed = vec![false; self.w * self.h];
        let heur = |i: i32, j: i32| -> u32 {
            let dx = (i - goal.0).unsigned_abs();
            let dy = (j - goal.1).unsigned_abs();
            let (mn, mx) = (dx.min(dy), dx.max(dy));
            mn * SQRT2_X16 + (mx - mn) * ONE_X16
        };
        let mut open: BinaryHeap<Reverse<(u32, u32, i32, i32)>> = BinaryHeap::new();
        g[idx(start.0, start.1)] = 0;
        open.push(Reverse((heur(start.0, start.1), 0, start.0, start.1)));
        let mut expansions = 0usize;
        let mut best_node = (heur(start.0, start.1), start.0, start.1);
        let mut found = false;
        while let Some(Reverse((_f, gc, ci, cj))) = open.pop() {
            let ic = idx(ci, cj);
            if closed[ic] {
                continue;
            }
            closed[ic] = true;
            if (ci, cj) == goal {
                found = true;
                break;
            }
            let hh = heur(ci, cj);
            if hh < best_node.0 {
                best_node = (hh, ci, cj);
            }
            expansions += 1;
            if expansions > max_expansions {
                break;
            }
            for dj in -1i32..=1 {
                for di in -1i32..=1 {
                    if di == 0 && dj == 0 {
                        continue;
                    }
                    let (ni, nj) = (ci + di, cj + dj);
                    if self.is_blocked(ni, nj) {
                        continue;
                    }
                    let diag = di != 0 && dj != 0;
                    if diag && (self.is_blocked(ci + di, cj) || self.is_blocked(ci, cj + dj)) {
                        continue; // no corner cutting
                    }
                    let Some(ex) = extra(ni, nj) else { continue };
                    let step = if diag { SQRT2_X16 } else { ONE_X16 };
                    let cost = step + (ex * ONE_X16 as f32) as u32;
                    let ng = gc + cost;
                    let ni_idx = idx(ni, nj);
                    if ng < g[ni_idx] {
                        g[ni_idx] = ng;
                        came[ni_idx] = ic as u32;
                        open.push(Reverse((ng + heur(ni, nj), ng, ni, nj)));
                    }
                }
            }
        }
        let end = if found { goal } else { (best_node.1, best_node.2) };
        if !found && (end == start) {
            return None;
        }
        let mut path = vec![];
        let mut cur = idx(end.0, end.1);
        loop {
            let (ci, cj) = ((cur % self.w) as i32, (cur / self.w) as i32);
            path.push(self.center(ci, cj));
            if (ci, cj) == start {
                break;
            }
            let p = came[cur];
            if p == u32::MAX {
                break;
            }
            cur = p as usize;
        }
        path.reverse();
        Some(path)
    }

    pub fn find_path(&self, from: Vec2, to: Vec2, max_expansions: usize) -> Option<Vec<Vec2>> {
        self.find_path_with(from, to, max_expansions, |_, _| Some(0.0))
    }

    /// String-pulling: drop waypoints whose neighbours can see each other.
    pub fn smooth(&self, path: &[Vec2]) -> Vec<Vec2> {
        if path.len() <= 2 {
            return path.to_vec();
        }
        let mut out = vec![path[0]];
        let mut i = 0;
        while i < path.len() - 1 {
            let mut j = path.len() - 1;
            while j > i + 1 && !self.line_clear(path[i], path[j]) {
                j -= 1;
            }
            out.push(path[j]);
            i = j;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> NavGrid {
        NavGrid::new(50.0, 2.0)
    }

    #[test]
    fn straight_path_on_empty_grid() {
        let g = grid();
        let p = g.find_path(Vec2::new(-40.0, -40.0), Vec2::new(40.0, 40.0), 100_000).unwrap();
        let s = g.smooth(&p);
        assert_eq!(s.len(), 2, "empty grid should smooth to a straight line");
        assert!(s[0].distance(Vec2::new(-40.0, -40.0)) < 3.0);
        assert!(s[1].distance(Vec2::new(40.0, 40.0)) < 3.0);
    }

    #[test]
    fn routes_around_wall() {
        let mut g = grid();
        // vertical wall at x ~ 0 from z=-30 to z=30
        for j in 10..40 {
            let (i, _) = g.cell_of(Vec2::new(0.0, 0.0));
            g.set_blocked(i, j, true);
        }
        let from = Vec2::new(-20.0, 0.0);
        let to = Vec2::new(20.0, 0.0);
        assert!(!g.line_clear(from, to));
        let p = g.find_path(from, to, 100_000).unwrap();
        let s = g.smooth(&p);
        assert!(s.len() >= 3, "path must bend around the wall: {s:?}");
        for w in s.windows(2) {
            assert!(g.line_clear(w[0], w[1]));
        }
        // Go around the end of the wall (z beyond ±30).
        assert!(s.iter().any(|q| q.y.abs() > 28.0));
    }

    #[test]
    fn unreachable_returns_partial_or_none() {
        let mut g = grid();
        // Enclose the goal.
        for j in 0..g.h as i32 {
            for i in 0..g.w as i32 {
                let c = g.center(i, j);
                let d = (c - Vec2::new(30.0, 30.0)).length();
                if (8.0..12.0).contains(&d) {
                    g.set_blocked(i, j, true);
                }
            }
        }
        let p = g.find_path(Vec2::new(-30.0, -30.0), Vec2::new(30.0, 30.0), 100_000);
        // Falls back to the closest reachable cell: ends outside the ring.
        let p = p.unwrap();
        assert!(p.last().unwrap().distance(Vec2::new(30.0, 30.0)) >= 7.0);
    }

    #[test]
    fn extra_cost_steers_path() {
        let g = grid();
        // Expensive band across the middle except a gap near z=+30.
        let cost = |i: i32, j: i32| -> Option<f32> {
            let c = g.center(i, j);
            if c.x.abs() < 4.0 && c.y < 26.0 {
                Some(50.0)
            } else {
                Some(0.0)
            }
        };
        let p = g.find_path_with(Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), 200_000, cost).unwrap();
        assert!(p.iter().any(|q| q.y > 20.0), "should detour through the cheap gap");
    }
}
