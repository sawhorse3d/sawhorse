//! A planar arrangement of directed segments that carry material evidence (lane 12d,
//! ADR-0010): how a section cap is assembled.
//!
//! This is the Sketch tier's planar-graph machinery (the same robust predicates,
//! [`crate::sketch::orient`] and friends, the same insertion rules and the same face walk,
//! [`crate::sketch::walk_circuits`]) with two differences that a cap needs:
//!
//! - **A tolerance fit for exact geometry.** A sketch merges within the 0.01 mm merge
//!   distance, which would move a cap's corners; here the tolerance is a few hundred
//!   rounding errors of the coordinates ([`Arrangement::new`]), and a point is never moved:
//!   an endpoint within the tolerance of another merges into it (the first one laid keeps
//!   its place), an endpoint within the tolerance of an edge splits that edge there (the
//!   edge bends by less than the tolerance), crossings split both segments at the crossing
//!   point, and a piece laid along an existing edge adds to it.
//! - **Material evidence on every edge.** Every segment is laid with a direction that has
//!   the material on its left, and with a vote (`+1`) or none (`0`). An edge keeps the signed
//!   sum of the votes laid along it; pieces laid both ways along one edge cancel.
//!
//! **What is material (lane 19c): the winding number.** The section cut lays the chords of a
//! closed surface's kept part (`crate::section`), which form a closed cycle by construction:
//! as many votes reach every point as leave it. Every step here keeps that so (merging two
//! vertices adds their edges, splitting an edge at a vertex or a crossing splits its votes
//! with it), so crossing an edge from its right to its left raises the winding number by the
//! votes along it, and every face of the arrangement has one winding number: the unbounded
//! face 0, every other face its neighbour's plus the votes between them
//! ([`Arrangement::material_rings`] works them out face to face from the unbounded one). A
//! face is material when its winding number is 1 or more. No oracle is asked: lane 12d's ray
//! cast into the solid (for faces whose votes disagreed) is gone, because for a cycle they
//! cannot disagree about the winding number. Only when the votes are not a cycle (a caller
//! laying segments that do not close, which the cut never does) are the numbers inconsistent;
//! then each face is decided by its own votes, the majority.
//!
//! The material faces' union is returned as closed rings with the material on their left.
//! Every ring closes by construction: around any vertex the boundary edges are exactly where
//! material and empty faces alternate, so as many boundary edges leave a vertex as reach it.
//! Where two material faces touch at a point (a pinch) the walk turns as far left as it can
//! and so traces one face at a time.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use glam::DVec2;

use crate::sketch::{
    line_intersection, orient, point_segment_distance, proper_cross, walk_circuits,
};
use crate::validate::point_in_ring;

/// One undirected edge `(lo, hi)` of the arrangement.
#[derive(Debug, Clone, Copy)]
struct Edge {
    /// The signed sum of the votes laid along it, positive along `lo -> hi`.
    net: i32,
    /// The first segment (by index) that laid it: rings start in segment order.
    first: usize,
}

/// See the module docs.
#[derive(Debug, Clone)]
pub(crate) struct Arrangement {
    tol: f64,
    pts: Vec<DVec2>,
    /// A vertex merged into another is dead: it has no edges and is never matched again.
    alive: Vec<bool>,
    edges: BTreeMap<(usize, usize), Edge>,
    /// Each segment as laid (by number): where a ring starts along it.
    segs: BTreeMap<usize, (DVec2, DVec2)>,
}

fn key(u: usize, v: usize) -> (usize, usize) {
    (u.min(v), u.max(v))
}

/// Counter-clockwise order of `p` and `q` around `o`, exactly (angles from +x in
/// `[0, 2 pi)`): the half-plane first, then the robust orientation.
fn ccw(o: DVec2, p: DVec2, q: DVec2) -> Ordering {
    let half = |x: DVec2| u8::from(!(x.y > o.y || (x.y == o.y && x.x > o.x)));
    half(p).cmp(&half(q)).then_with(|| {
        let s = orient(o, p, q);
        if s > 0.0 {
            Ordering::Less
        } else if s < 0.0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    })
}

impl Arrangement {
    /// An empty arrangement merging within `tol` (metres).
    pub(crate) fn new(tol: f64) -> Arrangement {
        Arrangement {
            tol,
            pts: Vec::new(),
            alive: Vec::new(),
            edges: BTreeMap::new(),
            segs: BTreeMap::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn edge_count(&self) -> usize {
        self.edges.len()
    }

    fn add_vertex(&mut self, p: DVec2) -> usize {
        self.pts.push(p);
        self.alive.push(true);
        self.pts.len() - 1
    }

    /// Lay `net` votes along `u -> v` (merging with an edge already there).
    fn add_edge(&mut self, u: usize, v: usize, net: i32, first: usize) {
        if u == v {
            return;
        }
        let sign = if u < v { 1 } else { -1 };
        let e = self
            .edges
            .entry(key(u, v))
            .or_insert(Edge { net: 0, first });
        e.net += sign * net;
        e.first = e.first.min(first);
    }

    /// Edge `e` through vertex `x` (which lies within the tolerance of it): two edges
    /// carrying the same votes and origin.
    fn split(&mut self, e: (usize, usize), x: usize) {
        let Some(d) = self.edges.remove(&e) else {
            return;
        };
        self.add_edge(e.0, x, d.net, d.first);
        self.add_edge(x, e.1, d.net, d.first);
    }

    /// The alive vertex nearest `p` within the tolerance (the lowest index on a tie).
    fn vertex_near(&self, p: DVec2) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for (i, q) in self.pts.iter().enumerate() {
            if !self.alive[i] {
                continue;
            }
            let d = q.distance(p);
            if d <= self.tol && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// The edge nearest `p` within the tolerance, not ending at `skip`.
    fn edge_near(&self, p: DVec2, skip: usize) -> Option<(usize, usize)> {
        let mut best: Option<(f64, (usize, usize))> = None;
        for &(a, b) in self.edges.keys() {
            if a == skip || b == skip {
                continue;
            }
            let d = point_segment_distance(p, self.pts[a], self.pts[b]);
            if d <= self.tol && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, (a, b)));
            }
        }
        best.map(|(_, e)| e)
    }

    /// The vertex for the point `p`: an existing one within the tolerance, else a new one at
    /// `p` exactly, splitting an edge it lies on. The section cut asks this once for every
    /// distinct end of its chords before laying any (lane 19c), so two chords that share an
    /// end always share its vertex and the votes stay a cycle.
    pub(crate) fn vertex(&mut self, p: DVec2) -> usize {
        if let Some(v) = self.vertex_near(p) {
            return v;
        }
        let v = self.add_vertex(p);
        if let Some(e) = self.edge_near(p, v) {
            self.split(e, v);
        }
        v
    }

    /// Lay the segment `a -> b` (material on its left) with `vote` (`1` or `0`), from
    /// segment number `first`.
    #[cfg(test)]
    pub(crate) fn insert(&mut self, a: DVec2, b: DVec2, vote: i32, first: usize) {
        self.segs.entry(first).or_insert((a, b));
        let p = self.vertex(a);
        let q = self.vertex(b);
        self.lay(p, q, vote, first);
    }

    /// Lay the segment from vertex `p` to vertex `q` (both from [`Arrangement::vertex`];
    /// material on its left) with `vote`, from segment number `first`.
    pub(crate) fn insert_between(&mut self, p: usize, q: usize, vote: i32, first: usize) {
        self.segs.entry(first).or_insert((self.pts[p], self.pts[q]));
        self.lay(p, q, vote, first);
    }

    /// Lay `p -> q`: split where existing vertices lie on it and where it properly crosses
    /// an existing edge (splitting that edge too), and add the votes along the pieces.
    fn lay(&mut self, p: usize, q: usize, vote: i32, first: usize) {
        if p == q {
            return;
        }
        let (pp, qq) = (self.pts[p], self.pts[q]);
        let d = qq - pp;
        let len2 = d.length_squared();
        let param = |x: DVec2| (x - pp).dot(d) / len2;
        // Vertices on the new segment split it.
        let mut cuts: Vec<(f64, usize)> = (0..self.pts.len())
            .filter(|&v| {
                v != p
                    && v != q
                    && self.alive[v]
                    && point_segment_distance(self.pts[v], pp, qq) <= self.tol
            })
            .map(|v| (param(self.pts[v]), v))
            .collect();
        // Proper crossings split both segments.
        let mut crossings: Vec<(f64, (usize, usize), DVec2)> = Vec::new();
        for &(c, e) in self.edges.keys() {
            if [c, e]
                .iter()
                .any(|&w| w == p || w == q || cuts.iter().any(|&(_, x)| x == w))
            {
                continue;
            }
            let (pc, pe) = (self.pts[c], self.pts[e]);
            if proper_cross(pp, qq, pc, pe) {
                let x = line_intersection(pp, qq, pc, pe);
                crossings.push((param(x), (c, e), x));
            }
        }
        crossings.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        for (t, e, x) in crossings {
            // A crossing within the tolerance of a vertex already there is that vertex.
            if let Some(w) = [e.0, e.1]
                .into_iter()
                .chain(cuts.iter().map(|&(_, w)| w))
                .find(|&w| self.pts[w].distance(x) <= self.tol)
            {
                if w != e.0 && w != e.1 {
                    self.split(e, w);
                }
                if !cuts.iter().any(|&(_, c)| c == w) {
                    cuts.push((param(self.pts[w]), w));
                }
                continue;
            }
            if x.distance(pp) <= self.tol || x.distance(qq) <= self.tol {
                continue;
            }
            let v = self.add_vertex(x);
            self.split(e, v);
            cuts.push((t, v));
        }
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        let chain: Vec<usize> = std::iter::once(p)
            .chain(cuts.into_iter().map(|(_, v)| v))
            .chain(std::iter::once(q))
            .collect();
        for w in chain.windows(2) {
            self.add_edge(w[0], w[1], vote, first);
        }
    }

    /// Settle what single insertions leave behind (a crossing that lands within the
    /// tolerance of an unrelated edge, two vertices closer than the tolerance), as the
    /// sketch's repair does: merge vertices, split an edge at a vertex on it, split proper
    /// crossings, until none is left (each step moves nothing by more than the tolerance,
    /// and each keeps the votes a cycle). A cluster that does not settle in 64 steps is left
    /// as it is: the faces are still walked, and the winding numbers say what they are.
    pub(crate) fn settle(&mut self) {
        for _ in 0..64 {
            if !self.repair_step() {
                return;
            }
        }
    }

    fn live_vertices(&self) -> Vec<usize> {
        let mut vs: Vec<usize> = self.edges.keys().flat_map(|&(a, b)| [a, b]).collect();
        vs.sort_unstable();
        vs.dedup();
        vs
    }

    fn repair_step(&mut self) -> bool {
        let vs = self.live_vertices();
        for (i, &a) in vs.iter().enumerate() {
            for &b in &vs[i + 1..] {
                if self.pts[a].distance(self.pts[b]) <= self.tol {
                    self.merge(b, a);
                    return true;
                }
            }
        }
        let edges: Vec<(usize, usize)> = self.edges.keys().copied().collect();
        for &(a, b) in &edges {
            let (pa, pb) = (self.pts[a], self.pts[b]);
            for &w in &vs {
                if w != a && w != b && point_segment_distance(self.pts[w], pa, pb) <= self.tol {
                    self.split((a, b), w);
                    return true;
                }
            }
        }
        for i in 0..edges.len() {
            for j in i + 1..edges.len() {
                let ((a, b), (c, d)) = (edges[i], edges[j]);
                if [a, b].iter().any(|x| *x == c || *x == d) {
                    continue;
                }
                let (pa, pb, pc, pd) = (self.pts[a], self.pts[b], self.pts[c], self.pts[d]);
                if proper_cross(pa, pb, pc, pd) {
                    let x = self.add_vertex(line_intersection(pa, pb, pc, pd));
                    self.split((a, b), x);
                    self.split((c, d), x);
                    return true;
                }
            }
        }
        false
    }

    /// Vertex `from` into `into`: its edges move over (an edge between the two goes).
    fn merge(&mut self, from: usize, into: usize) {
        let touched: Vec<((usize, usize), Edge)> = self
            .edges
            .iter()
            .filter(|(k, _)| k.0 == from || k.1 == from)
            .map(|(k, e)| (*k, *e))
            .collect();
        for (k, e) in touched {
            self.edges.remove(&k);
            let (a, b) = (
                if k.0 == from { into } else { k.0 },
                if k.1 == from { into } else { k.1 },
            );
            self.add_edge(a, b, e.net, e.first);
        }
        self.alive[from] = false;
    }

    /// The signed votes along `u -> v`.
    fn net(&self, u: usize, v: usize) -> i32 {
        let e = self.edges[&key(u, v)];
        if u < v { e.net } else { -e.net }
    }

    /// Every vertex's neighbours, counter-clockwise (exactly).
    fn adjacency(&self) -> BTreeMap<usize, Vec<usize>> {
        let mut adj: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &(a, b) in self.edges.keys() {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
        for (v, nbrs) in adj.iter_mut() {
            let o = self.pts[*v];
            nbrs.sort_by(|p, q| ccw(o, self.pts[*p], self.pts[*q]).then(p.cmp(q)));
        }
        adj
    }

    fn ring(&self, c: &[usize]) -> Vec<DVec2> {
        c.iter().map(|&v| self.pts[v]).collect()
    }

    /// The faces of the arrangement, which of them are material (winding number 1 or more,
    /// see the module docs), and the boundary of their union as rings of vertices with the
    /// material on their left.
    pub(crate) fn material_rings(&self) -> Vec<Vec<DVec2>> {
        if self.edges.is_empty() {
            return Vec::new();
        }
        let adj = self.adjacency();
        let circuits = walk_circuits(&adj);
        // Connected components (circuits of one component never nest in each other).
        let mut comp: BTreeMap<usize, usize> = BTreeMap::new();
        for &v in adj.keys() {
            if comp.contains_key(&v) {
                continue;
            }
            let id = v;
            let mut stack = vec![v];
            comp.insert(v, id);
            while let Some(x) = stack.pop() {
                for &y in &adj[&x] {
                    if let std::collections::btree_map::Entry::Vacant(e) = comp.entry(y) {
                        e.insert(id);
                        stack.push(y);
                    }
                }
            }
        }
        let rings: Vec<Vec<DVec2>> = circuits.iter().map(|c| self.ring(c)).collect();
        let area: Vec<f64> = rings.iter().map(|r| centred_area(r)).collect();
        // A counter-clockwise circuit bounds a face from outside; any other circuit is a
        // hole of the smallest counter-clockwise circuit of another component around it, or
        // part of the unbounded face (`None`).
        let outers: Vec<usize> = (0..circuits.len()).filter(|&i| area[i] > 0.0).collect();
        let face_of: Vec<Option<usize>> = (0..circuits.len())
            .map(|i| {
                if area[i] > 0.0 {
                    return Some(i);
                }
                let probe = rings[i][0];
                let me = comp[&circuits[i][0]];
                outers
                    .iter()
                    .copied()
                    .filter(|&o| comp[&circuits[o][0]] != me && point_in_ring(probe, &rings[o]))
                    .min_by(|&x, &y| area[x].total_cmp(&area[y]).then(x.cmp(&y)))
            })
            .collect();
        let material = self
            .windings(&circuits, &face_of)
            .map(|w| {
                outers
                    .iter()
                    .map(|&f| (f, w[f + 1] >= 1))
                    .collect::<BTreeMap<usize, bool>>()
            })
            .unwrap_or_else(|| self.by_votes(&circuits, &face_of, &outers));
        // Each directed edge's side: material on its left?
        let mut left: BTreeMap<(usize, usize), bool> = BTreeMap::new();
        for (i, c) in circuits.iter().enumerate() {
            let m = face_of[i].is_some_and(|f| material[&f]);
            for k in 0..c.len() {
                left.insert((c[k], c[(k + 1) % c.len()]), m);
            }
        }
        let boundary = |u: usize, v: usize| left[&(u, v)] && !left[&(v, u)];
        // Walk the boundary, starting in the order the segments were laid.
        let mut starts: Vec<(usize, f64, usize, usize)> = Vec::new();
        for (&(a, b), e) in &self.edges {
            let (u, v) = if boundary(a, b) {
                (a, b)
            } else if boundary(b, a) {
                (b, a)
            } else {
                continue;
            };
            let (sa, sb) = self.segs.get(&e.first).copied().unwrap_or_default();
            let along = (self.pts[u] - sa).dot(sb - sa);
            starts.push((e.first, along, u, v));
        }
        starts.sort_by(|x, y| {
            x.0.cmp(&y.0)
                .then(x.1.total_cmp(&y.1))
                .then((x.2, x.3).cmp(&(y.2, y.3)))
        });
        // Rings start where the segment that laid their first edge started: within one
        // segment, at its start.
        let mut used: std::collections::BTreeSet<(usize, usize)> = Default::default();
        let mut out = Vec::new();
        let limit = 2 * self.edges.len() + 2;
        for &(_, _, u0, v0) in &starts {
            if used.contains(&(u0, v0)) {
                continue;
            }
            let mut ring = Vec::new();
            let (mut u, mut v) = (u0, v0);
            loop {
                used.insert((u, v));
                ring.push(u);
                // The boundary edge leaving v just clockwise of v -> u (the leftmost turn).
                let nbrs = &adj[&v];
                let i = nbrs.iter().position(|&w| w == u).expect("twin exists");
                let n = nbrs.len();
                let Some(w) = (1..=n)
                    .map(|k| nbrs[(i + n * 2 - k) % n])
                    .find(|&w| boundary(v, w))
                else {
                    break; // cannot happen: boundary edges alternate around a vertex
                };
                (u, v) = (v, w);
                if (u, v) == (u0, v0) || ring.len() > limit {
                    break;
                }
            }
            out.push(ring);
        }
        out.into_iter()
            .flat_map(split_pinches)
            .map(|r| self.ring(&r))
            .collect()
    }

    /// The winding number of every face (index 0: the unbounded face; `1 + o`: the face
    /// bounded from outside by circuit `o`), worked out face to face from the unbounded one
    /// across every edge (the face on an edge's left has the one on its right's number plus
    /// the votes along it). `None` when two ways round give a face two numbers: the votes are
    /// not a cycle.
    fn windings(&self, circuits: &[Vec<usize>], face_of: &[Option<usize>]) -> Option<Vec<i64>> {
        let face = |i: usize| face_of[i].map_or(0, |f| f + 1);
        let mut holder: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for (i, c) in circuits.iter().enumerate() {
            for k in 0..c.len() {
                holder.insert((c[k], c[(k + 1) % c.len()]), i);
            }
        }
        // Each face's neighbours: (the face across, its number minus this face's).
        let mut links: Vec<Vec<(usize, i64)>> = vec![Vec::new(); circuits.len() + 1];
        for (&(u, v), &i) in &holder {
            let j = *holder.get(&(v, u))?;
            links[face(j)].push((face(i), i64::from(self.net(u, v))));
        }
        let mut w: Vec<Option<i64>> = vec![None; circuits.len() + 1];
        w[0] = Some(0);
        let mut queue = std::collections::VecDeque::from([0usize]);
        while let Some(f) = queue.pop_front() {
            let here = w[f].expect("queued faces have a number");
            for &(g, d) in &links[f] {
                match w[g] {
                    None => {
                        w[g] = Some(here + d);
                        queue.push_back(g);
                    }
                    Some(x) if x != here + d => return None,
                    Some(_) => {}
                }
            }
        }
        // A face nothing reaches (it cannot happen: the faces of a plane are connected
        // through the edges between them) counts as empty.
        Some(w.into_iter().map(|x| x.unwrap_or(0)).collect())
    }

    /// Material by each face's own votes, when the votes are not a cycle: every edge around
    /// it that has a vote says so, or most of them do.
    fn by_votes(
        &self,
        circuits: &[Vec<usize>],
        face_of: &[Option<usize>],
        outers: &[usize],
    ) -> BTreeMap<usize, bool> {
        let mut votes: BTreeMap<usize, (u32, u32)> = BTreeMap::new();
        for (i, c) in circuits.iter().enumerate() {
            let Some(f) = face_of[i] else {
                continue;
            };
            let v = votes.entry(f).or_default();
            for k in 0..c.len() {
                match self.net(c[k], c[(k + 1) % c.len()]).cmp(&0) {
                    Ordering::Greater => v.0 += 1,
                    Ordering::Less => v.1 += 1,
                    Ordering::Equal => {}
                }
            }
        }
        outers
            .iter()
            .map(|&f| {
                let (yes, no) = votes.get(&f).copied().unwrap_or_default();
                (f, yes > no)
            })
            .collect()
    }

    /// Rings as point lists, with the order the start was found in (tests).
    #[cfg(test)]
    pub(crate) fn rings_for_test(&self) -> Vec<Vec<DVec2>> {
        self.material_rings()
    }
}

/// The shoelace area of a ring about its first point (less cancellation far from the
/// origin).
fn centred_area(r: &[DVec2]) -> f64 {
    let o = r[0];
    let n = r.len();
    let mut s = 0.0;
    for i in 0..n {
        let (a, b) = (r[i] - o, r[(i + 1) % n] - o);
        s += a.perp_dot(b);
    }
    s * 0.5
}

/// A closed ring that passes a vertex more than once, as the simple rings it is made of
/// (each from one visit of the vertex to the next).
pub(crate) fn split_pinches<T: PartialEq + Copy>(ring: Vec<T>) -> Vec<Vec<T>> {
    let mut out = Vec::new();
    let mut stack: Vec<T> = Vec::with_capacity(ring.len());
    for k in ring {
        if let Some(i) = stack.iter().position(|x| *x == k) {
            out.push(stack.split_off(i));
        }
        stack.push(k);
    }
    out.push(stack);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn area(r: &[DVec2]) -> f64 {
        sawhorse_tessellate::signed_area(r)
    }

    fn square(a: &mut Arrangement, x0: f64, y0: f64, x1: f64, y1: f64, first: usize) {
        let c = [v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)];
        for i in 0..4 {
            a.insert(c[i], c[(i + 1) % 4], 1, first + i);
        }
    }

    #[test]
    fn a_square_is_one_ring_from_its_first_segment() {
        let mut a = Arrangement::new(1e-12);
        square(&mut a, 0.0, 0.0, 2.0, 1.0, 0);
        a.settle();
        let rs = a.rings_for_test();
        assert_eq!(
            rs,
            vec![vec![v(0.0, 0.0), v(2.0, 0.0), v(2.0, 1.0), v(0.0, 1.0)]]
        );
    }

    #[test]
    fn a_hole_along_the_outline_makes_a_notch() {
        // The outline's top edge runs straight across; the hole's top edge lies along it,
        // the other way: they cancel and the cap is a U.
        let mut a = Arrangement::new(1e-12);
        let pts = [v(0.0, 0.0), v(3.0, 0.0), v(3.0, 2.0), v(0.0, 2.0)];
        for i in 0..4 {
            a.insert(pts[i], pts[(i + 1) % 4], 1, i);
        }
        let hole = [v(1.0, 2.0), v(2.0, 2.0), v(2.0, 1.0), v(1.0, 1.0)];
        for i in 0..4 {
            a.insert(hole[i], hole[(i + 1) % 4], 1, 4 + i);
        }
        a.settle();
        let rs = a.rings_for_test();
        assert_eq!(rs.len(), 1, "{rs:?}");
        assert!((area(&rs[0]) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn two_squares_touching_at_a_corner_are_two_rings() {
        let mut a = Arrangement::new(1e-12);
        square(&mut a, 0.0, 0.0, 1.0, 1.0, 0);
        square(&mut a, 1.0, 1.0, 2.0, 2.0, 4);
        a.settle();
        let rs = a.rings_for_test();
        assert_eq!(rs.len(), 2);
        for r in &rs {
            assert_eq!(r.len(), 4);
            assert!((area(r) - 1.0).abs() < 1e-12);
        }
    }

    /// Lane 19c: votes that are not a cycle (a segment laid backwards) give a face two winding
    /// numbers; the face is then decided by its own votes, three to one.
    #[test]
    fn votes_that_do_not_close_are_decided_by_the_majority() {
        let mut a = Arrangement::new(1e-12);
        let c = [v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)];
        a.insert(c[0], c[1], 1, 0);
        a.insert(c[1], c[2], 1, 1);
        a.insert(c[3], c[2], 1, 2); // backwards
        a.insert(c[3], c[0], 1, 3);
        a.settle();
        let rs = a.rings_for_test();
        assert_eq!(rs.len(), 1);
        assert!((area(&rs[0]) - 1.0).abs() < 1e-12);
    }

    /// Lane 19c: a cycle that winds twice round part of the plane (two squares overlapping,
    /// both counter-clockwise) and one that winds the wrong way (a clockwise square alone):
    /// material is a winding number of 1 or more, so the first is the union of the two
    /// squares and the second is nothing.
    #[test]
    fn material_is_a_winding_number_of_one_or_more() {
        let mut a = Arrangement::new(1e-12);
        square(&mut a, 0.0, 0.0, 2.0, 2.0, 0);
        square(&mut a, 1.0, 1.0, 3.0, 3.0, 4);
        a.settle();
        let rs = a.rings_for_test();
        let total: f64 = rs.iter().map(|r| area(r)).sum();
        assert!((total - 7.0).abs() < 1e-12, "{rs:?}");
        let mut b = Arrangement::new(1e-12);
        let c = [v(0.0, 0.0), v(0.0, 1.0), v(1.0, 1.0), v(1.0, 0.0)];
        for i in 0..4 {
            b.insert(c[i], c[(i + 1) % 4], 1, i);
        }
        b.settle();
        assert!(b.rings_for_test().is_empty());
    }

    /// Lane 19c: chords laid between vertices asked for once each (as the cut lays them): a
    /// point within the tolerance of one already there is that vertex, so the votes close.
    #[test]
    fn chords_between_shared_vertices_close() {
        let mut a = Arrangement::new(1e-9);
        let pts = [v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)];
        let ids: Vec<usize> = pts.iter().map(|p| a.vertex(*p)).collect();
        // A point a hair from the second corner is the second corner.
        assert_eq!(a.vertex(v(1.0 + 1e-10, 0.0)), ids[1]);
        for i in 0..4 {
            a.insert_between(ids[i], ids[(i + 1) % 4], 1, i);
        }
        a.settle();
        let rs = a.rings_for_test();
        assert_eq!(rs, vec![pts.to_vec()]);
    }

    #[test]
    fn crossings_and_t_junctions_split() {
        let mut a = Arrangement::new(1e-12);
        a.insert(v(0.0, 0.0), v(2.0, 2.0), 1, 0);
        a.insert(v(0.0, 2.0), v(2.0, 0.0), 1, 1);
        assert_eq!(a.edge_count(), 4);
        a.insert(v(1.0, 0.0), v(1.0, 1.0 + 1e-14), 1, 2); // ends on the crossing
        a.settle();
        assert_eq!(a.edge_count(), 5);
    }

    #[test]
    fn nested_holes_and_islands() {
        let mut a = Arrangement::new(1e-12);
        square(&mut a, 0.0, 0.0, 10.0, 10.0, 0);
        // A hole (clockwise) and an island inside it.
        let h = [v(2.0, 2.0), v(2.0, 8.0), v(8.0, 8.0), v(8.0, 2.0)];
        for i in 0..4 {
            a.insert(h[i], h[(i + 1) % 4], 1, 4 + i);
        }
        square(&mut a, 4.0, 4.0, 6.0, 6.0, 8);
        a.settle();
        let rs = a.rings_for_test();
        let total: f64 = rs.iter().map(|r| area(r)).sum();
        assert!((total - (100.0 - 36.0 + 4.0)).abs() < 1e-9, "{rs:?}");
        assert_eq!(rs.len(), 3);
    }
}
