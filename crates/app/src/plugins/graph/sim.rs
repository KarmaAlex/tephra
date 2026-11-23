//! Force-directed layout for the graph view, after d3-force (which Obsidian's graph is modelled
//! on): many-body repulsion (Barnes–Hut quadtree, O(n log n)), link springs, a pull to the
//! centre, velocity decay and a cooling `alpha`. Pure and GPU-free, so it is unit-tested.

/// Obsidian's "Forces" settings (`.obsidian/graph.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forces {
    /// "Center force", 0–1.
    pub center: f32,
    /// "Repel force", 0–20.
    pub repel: f32,
    /// "Link force", 0–1.
    pub link: f32,
    /// "Link distance", 30–500.
    pub distance: f32,
}

impl Default for Forces {
    fn default() -> Self {
        Self { center: 0.52, repel: 10.0, link: 1.0, distance: 250.0 }
    }
}

const THETA2: f32 = 0.81; // Barnes–Hut accuracy (theta 0.9, as d3)
const VELOCITY_DECAY: f32 = 0.4;
const ALPHA_MIN: f32 = 0.001;

pub struct Sim {
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    vx: Vec<f32>,
    vy: Vec<f32>,
    /// Pinned positions (a node being dragged).
    pub fixed: Vec<Option<(f32, f32)>>,
    links: Vec<(usize, usize)>,
    /// Per link: strength, and how much of the correction the target takes.
    link_k: Vec<(f32, f32)>,
    pub alpha: f32,
    alpha_target: f32,
    alpha_decay: f32,
    pub forces: Forces,
    /// Many-body strength per node (negative: repulsion).
    charge: f32,
}

/// d3's initial layout: a phyllotaxis spiral, so nodes start spread and deterministic.
pub fn spiral(i: usize) -> (f32, f32) {
    let r = 10.0 * (0.5 + i as f32).sqrt();
    let a = i as f32 * std::f32::consts::PI * (3.0 - 5f32.sqrt());
    (r * a.cos(), r * a.sin())
}

#[derive(Clone, Copy)]
struct Quad {
    /// Centre of mass and mass (number of nodes).
    cx: f32,
    cy: f32,
    mass: f32,
    x0: f32,
    y0: f32,
    size: f32,
    /// First child index (4 consecutive), 0 = leaf.
    kids: u32,
    /// The single node of a leaf, or u32::MAX (empty, or several coincident nodes).
    point: u32,
}

struct Tree {
    q: Vec<Quad>,
}

impl Tree {
    fn build(x: &[f32], y: &[f32]) -> Self {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for i in 0..x.len() {
            x0 = x0.min(x[i]);
            y0 = y0.min(y[i]);
            x1 = x1.max(x[i]);
            y1 = y1.max(y[i]);
        }
        let size = (x1 - x0).max(y1 - y0).max(1.0) * 1.0001;
        let mut t = Tree { q: Vec::with_capacity(x.len() * 3) };
        t.q.push(Quad { cx: 0., cy: 0., mass: 0., x0, y0, size, kids: 0, point: u32::MAX });
        for i in 0..x.len() {
            t.insert(0, i as u32, x[i], y[i], x, y, 0);
        }
        t
    }

    fn insert(&mut self, at: usize, i: u32, px: f32, py: f32, x: &[f32], y: &[f32], depth: u32) {
        let q = self.q[at];
        // update the centre of mass on the way down
        let m = q.mass + 1.0;
        self.q[at].cx = (q.cx * q.mass + px) / m;
        self.q[at].cy = (q.cy * q.mass + py) / m;
        self.q[at].mass = m;
        if q.kids == 0 {
            if q.mass == 0.0 {
                self.q[at].point = i;
                return;
            }
            // coincident points (or too deep): keep them together in this leaf
            if depth > 24 {
                self.q[at].point = u32::MAX;
                return;
            }
            let old = q.point;
            self.split(at);
            if old != u32::MAX {
                let (ox, oy) = (x[old as usize], y[old as usize]);
                let c = self.child_for(at, ox, oy);
                self.insert(c, old, ox, oy, x, y, depth + 1);
            }
            self.q[at].point = u32::MAX;
        }
        let c = self.child_for(at, px, py);
        self.insert(c, i, px, py, x, y, depth + 1);
    }

    fn split(&mut self, at: usize) {
        let q = self.q[at];
        let h = q.size / 2.0;
        let first = self.q.len() as u32;
        for k in 0..4 {
            let (dx, dy) = ((k & 1) as f32 * h, (k >> 1) as f32 * h);
            self.q.push(Quad { cx: 0., cy: 0., mass: 0., x0: q.x0 + dx, y0: q.y0 + dy, size: h, kids: 0, point: u32::MAX });
        }
        self.q[at].kids = first;
    }

    fn child_for(&self, at: usize, px: f32, py: f32) -> usize {
        let q = self.q[at];
        let h = q.size / 2.0;
        let k = usize::from(px >= q.x0 + h) + 2 * usize::from(py >= q.y0 + h);
        q.kids as usize + k
    }
}

impl Sim {
    /// `n` nodes, initial positions from `start` (None: the spiral), and links (i, j).
    pub fn new(n: usize, start: &dyn Fn(usize) -> Option<(f32, f32)>, links: Vec<(usize, usize)>, forces: Forces) -> Self {
        let (mut x, mut y) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let (a, b) = start(i).unwrap_or_else(|| spiral(i));
            x.push(a);
            y.push(b);
        }
        let mut sim = Self { x, y, vx: vec![0.; n], vy: vec![0.; n], fixed: vec![None; n], links: Vec::new(), link_k: Vec::new(), alpha: 1.0, alpha_target: 0.0, alpha_decay: 1.0 - ALPHA_MIN.powf(1.0 / 300.0), forces, charge: 0.0 };
        sim.set_links(links);
        sim.set_forces(forces);
        sim
    }

    pub fn len(&self) -> usize {
        self.x.len()
    }

    fn set_links(&mut self, links: Vec<(usize, usize)>) {
        let mut count = vec![0f32; self.len()];
        for &(a, b) in &links {
            count[a] += 1.0;
            count[b] += 1.0;
        }
        self.link_k = links.iter().map(|&(a, b)| (1.0 / count[a].min(count[b]).max(1.0), count[a] / (count[a] + count[b]))).collect();
        self.links = links;
    }

    pub fn set_forces(&mut self, f: Forces) {
        self.forces = f;
        // Obsidian's 0–20 repel scale onto d3 strengths (d3's default is -30)
        self.charge = -f.repel * 18.0;
    }

    pub fn running(&self) -> bool {
        self.alpha >= ALPHA_MIN || self.alpha_target > 0.0
    }

    /// Warms the layout up again (after a drag or a change), to at least `a`.
    pub fn reheat(&mut self, a: f32) {
        self.alpha = self.alpha.max(a);
    }

    /// While dragging, keep the layout moving (d3's alphaTarget).
    pub fn set_alpha_target(&mut self, t: f32) {
        self.alpha_target = t;
        if t > 0.0 {
            self.alpha = self.alpha.max(t);
        }
    }

    pub fn tick(&mut self) {
        let n = self.len();
        if n == 0 {
            return;
        }
        self.alpha += (self.alpha_target - self.alpha) * self.alpha_decay;
        let alpha = self.alpha;
        // links (spring towards the link distance)
        let (dist, ks) = (self.forces.distance, self.forces.link);
        for (li, &(s, t)) in self.links.iter().enumerate() {
            if s == t {
                continue;
            }
            let (strength, bias) = self.link_k[li];
            let mut dx = self.x[t] + self.vx[t] - self.x[s] - self.vx[s];
            let mut dy = self.y[t] + self.vy[t] - self.y[s] - self.vy[s];
            if dx == 0.0 && dy == 0.0 {
                dx = 1e-3 * ((li % 7) as f32 - 3.0);
                dy = 1e-3;
            }
            let l = (dx * dx + dy * dy).sqrt();
            let k = (l - dist) / l * alpha * strength * ks;
            dx *= k;
            dy *= k;
            self.vx[t] -= dx * bias;
            self.vy[t] -= dy * bias;
            self.vx[s] += dx * (1.0 - bias);
            self.vy[s] += dy * (1.0 - bias);
        }
        // many-body repulsion
        let tree = Tree::build(&self.x, &self.y);
        let charge = self.charge;
        let mut stack: Vec<usize> = Vec::with_capacity(64);
        for i in 0..n {
            let (xi, yi) = (self.x[i], self.y[i]);
            let (mut fx, mut fy) = (0.0f32, 0.0f32);
            stack.clear();
            stack.push(0);
            while let Some(at) = stack.pop() {
                let q = tree.q[at];
                if q.mass == 0.0 {
                    continue;
                }
                let (mut dx, mut dy) = (q.cx - xi, q.cy - yi);
                let mut d2 = dx * dx + dy * dy;
                let leaf = q.kids == 0;
                if !leaf && q.size * q.size >= THETA2 * d2 {
                    let k = q.kids as usize;
                    stack.extend([k, k + 1, k + 2, k + 3]);
                    continue;
                }
                if leaf && q.point == i as u32 {
                    continue;
                }
                let mut mass = q.mass;
                if leaf && q.point == u32::MAX && d2 < 1e-6 {
                    // coincident with this node: everyone but itself, nudged apart
                    mass -= 1.0;
                    dx = ((i * 7919) % 13) as f32 - 6.0;
                    dy = ((i * 104729) % 11) as f32 - 5.0;
                    d2 = dx * dx + dy * dy;
                }
                let d2 = d2.max(1.0);
                let w = charge * mass * alpha / d2;
                fx += dx * w;
                fy += dy * w;
            }
            self.vx[i] += fx;
            self.vy[i] += fy;
        }
        // pull to the centre (Obsidian's "center force")
        let c = self.forces.center * 0.1 * alpha;
        for i in 0..n {
            self.vx[i] -= self.x[i] * c;
            self.vy[i] -= self.y[i] * c;
        }
        for i in 0..n {
            match self.fixed[i] {
                Some((fx, fy)) => {
                    self.x[i] = fx;
                    self.y[i] = fy;
                    self.vx[i] = 0.0;
                    self.vy[i] = 0.0;
                }
                None => {
                    self.vx[i] *= 1.0 - VELOCITY_DECAY;
                    self.vy[i] *= 1.0 - VELOCITY_DECAY;
                    self.x[i] += self.vx[i];
                    self.y[i] += self.vy[i];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settles_without_overlap_or_nan() {
        // a star of 10 around node 0, plus 30 orphans
        let links: Vec<(usize, usize)> = (1..=10).map(|i| (0, i)).collect();
        let mut s = Sim::new(41, &|_| None, links, Forces::default());
        let mut ticks = 0;
        while s.running() && ticks < 1000 {
            s.tick();
            ticks += 1;
        }
        assert!(!s.running() && ticks < 400, "cools down in ~300 ticks ({ticks})");
        assert!(s.x.iter().chain(&s.y).all(|v| v.is_finite()));
        // linked nodes sit near the link distance; nobody is on top of anybody
        for i in 1..=10 {
            let d = ((s.x[i] - s.x[0]).powi(2) + (s.y[i] - s.y[0]).powi(2)).sqrt();
            assert!(d > 80.0 && d < 500.0, "leaf {i} at {d}");
        }
        for i in 0..41 {
            for j in i + 1..41 {
                let d = ((s.x[i] - s.x[j]).powi(2) + (s.y[i] - s.y[j]).powi(2)).sqrt();
                assert!(d > 5.0, "{i} and {j} overlap ({d})");
            }
        }
    }

    #[test]
    fn barnes_hut_matches_exact_sum() {
        // a far cluster seen from one node: the approximation stays close to the exact force
        let n = 200;
        let mut s = Sim::new(n, &|i| Some(if i == 0 { (0.0, 0.0) } else { (1000.0 + (i % 10) as f32 * 3.0, (i / 10) as f32 * 3.0) }), vec![], Forces { center: 0.0, ..Forces::default() });
        let (x, y) = (s.x.clone(), s.y.clone());
        s.tick();
        // node 0's displacement direction: straight away from the cluster (-x), magnitude ≈ exact
        let mut exact = 0.0f32;
        for i in 1..n {
            let (dx, dy) = (x[i], y[i]);
            exact += dx * s.charge * s.alpha / (dx * dx + dy * dy);
        }
        let moved = s.x[0] / (1.0 - VELOCITY_DECAY);
        assert!(moved < 0.0 && ((moved - exact) / exact).abs() < 0.05, "BH {moved} vs exact {exact}");
    }

    #[test]
    fn coincident_nodes_separate() {
        let mut s = Sim::new(5, &|_| Some((3.0, 3.0)), vec![], Forces::default());
        for _ in 0..50 {
            s.tick();
        }
        assert!(s.x.iter().chain(&s.y).all(|v| v.is_finite()));
        let spread = s.x.iter().cloned().fold(f32::MIN, f32::max) - s.x.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread > 10.0, "{spread}");
    }
}
