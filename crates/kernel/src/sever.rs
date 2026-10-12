//! Severance by connectivity (lane 4): a cut that goes all the way through a board leaves
//! two pieces, and each piece is its own solid.
//!
//! A solid's faces fall into shells (faces connected through shared edges). A shell
//! enclosing positive volume is a piece; a shell enclosing negative volume is a void (a
//! cavity) and stays with the smallest piece that contains it. When a solid has two or
//! more pieces, [`Object::split_shells`] returns one solid per piece: the piece holding the
//! smallest face id comes first (the model keeps the original object id and display number
//! for it), the rest follow ordered by their smallest face id. Every face keeps its
//! persistent id (ids are unique within the old solid, so they are unique within each
//! piece), user-drawn edges stay drawn, and every piece keeps the face-id counter so no id
//! is ever handed out twice.

use std::collections::BTreeMap;

use glam::DVec3;

use crate::boolean::polys_tri_solid;
use crate::brep::Polys;
use crate::object::Object;

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// The faces of `p` grouped into edge-connected shells (each sorted; shells ordered by
/// their first face).
pub fn shells(p: &Polys) -> Vec<Vec<usize>> {
    let directed = p.directed_edges();
    let mut parent: Vec<usize> = (0..p.faces.len()).collect();
    for (&(a, b), &(fa, _)) in &directed {
        if let Some(&(fb, _)) = directed.get(&(b, a)) {
            let (ra, rb) = (find(&mut parent, fa), find(&mut parent, fb));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for f in 0..p.faces.len() {
        let r = find(&mut parent, f);
        groups.entry(r).or_default().push(f);
    }
    groups.into_values().collect()
}

/// Only the faces `faces` of `p`, unused points dropped.
fn subset(p: &Polys, faces: &[usize]) -> Polys {
    let mut q = Polys {
        points: p.points.clone(),
        faces: faces.iter().map(|&f| p.faces[f].clone()).collect(),
        user_edges: p.user_edges.clone(),
    };
    q.compact();
    q
}

fn volume(p: &Polys) -> f64 {
    polys_tri_solid(p, 0).map(|t| t.volume()).unwrap_or(0.0)
}

/// Point-in-solid by ray parity (a fixed, generic direction so the ray misses edges).
pub(crate) fn contains(p: &Polys, q: DVec3) -> bool {
    let Ok(t) = polys_tri_solid(p, 0) else {
        return false;
    };
    let dir = DVec3::new(0.577_215_664_9, 0.618_033_988_7, 0.533_012_798_1).normalize();
    let mut hits = 0;
    for tri in &t.triangles {
        let [a, b, c] = tri.map(|i| t.positions[i as usize]);
        // Moller-Trumbore.
        let e1 = b - a;
        let e2 = c - a;
        let h = dir.cross(e2);
        let det = e1.dot(h);
        if det.abs() < 1e-18 {
            continue;
        }
        let f = 1.0 / det;
        let s = q - a;
        let u = f * s.dot(h);
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let qv = s.cross(e1);
        let v = f * dir.dot(qv);
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        if f * e2.dot(qv) > 0.0 {
            hits += 1;
        }
    }
    hits % 2 == 1
}

/// Pieces of `p` (positive shells with the voids inside them), or `None` with fewer than two.
pub fn pieces(p: &Polys) -> Option<Vec<Vec<usize>>> {
    let all = shells(p);
    if all.len() < 2 {
        return None;
    }
    let vols: Vec<f64> = all.iter().map(|s| volume(&subset(p, s))).collect();
    let solid: Vec<usize> = (0..all.len()).filter(|&i| vols[i] > 0.0).collect();
    if solid.len() < 2 {
        return None;
    }
    let mut out: Vec<Vec<usize>> = solid.iter().map(|&i| all[i].clone()).collect();
    for (i, s) in all.iter().enumerate() {
        if vols[i] > 0.0 {
            continue;
        }
        // A void: into the smallest piece containing one of its points.
        let probe = p.points[p.faces[s[0]].outer[0]];
        let host = (0..solid.len())
            .filter(|&k| contains(&subset(p, &all[solid[k]]), probe))
            .min_by(|&x, &y| vols[solid[x]].total_cmp(&vols[solid[y]]))
            .unwrap_or(0);
        out[host].extend(s.iter().copied());
    }
    for piece in &mut out {
        piece.sort_unstable();
    }
    Some(out)
}

impl Object {
    /// Number of shells enclosing positive volume (1 for every committed solid).
    pub fn piece_count(&self) -> usize {
        let p = self.brep.to_polys();
        shells(&p)
            .iter()
            .filter(|s| volume(&subset(&p, s)) > 0.0)
            .count()
    }

    /// One solid per piece when this one has come apart (see the module docs), else `None`.
    pub fn split_shells(&self) -> Option<Vec<Object>> {
        let p = self.brep.to_polys();
        let mut groups = pieces(&p)?;
        let min_id = |g: &Vec<usize>| g.iter().filter_map(|&f| p.faces[f].id).min();
        groups.sort_by_key(|g| min_id(g));
        let mut out = Vec::with_capacity(groups.len());
        for (k, g) in groups.iter().enumerate() {
            let mut o = self.rebuilt(subset(&p, g)).ok()?;
            o.next_face_id = self.next_face_id;
            if k > 0 {
                // A new solid: no display number yet (the model gives it one) and no source
                // sketch (collapsing it never returns a footprint).
                o.source = None;
                o.number = 0;
                // Inside a component it is a new member of the definition (see nodes.rs).
                o.member.slot = None;
            }
            out.push(o);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plane::Plane;
    use glam::DVec2;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x, y),
            DVec2::new(x + w, y),
            DVec2::new(x + w, y + h),
            DVec2::new(x, y + h),
        ]
    }

    #[test]
    fn one_piece_does_not_split() {
        let o = Object::extrude(&Plane::GROUND, &rect(0.0, 0.0, 1.0, 1.0), &[], 1.0).unwrap();
        assert_eq!(o.piece_count(), 1);
        assert!(o.split_shells().is_none());
    }

    #[test]
    fn a_hollow_box_is_one_piece() {
        // An outer box with an inner, inward-facing box: a void, not a second piece.
        let outer = Object::extrude(&Plane::GROUND, &rect(0.0, 0.0, 4.0, 4.0), &[], 4.0).unwrap();
        let inner = Object::extrude(&Plane::GROUND, &rect(1.0, 1.0, 2.0, 2.0), &[], 2.0).unwrap();
        let mut p = outer.brep.to_polys();
        let q = inner.brep.to_polys();
        let base = p.points.len();
        p.points.extend(q.points.iter().map(|&x| x + DVec3::Z));
        for f in &q.faces {
            let mut f = f.clone();
            f.outer.reverse();
            f.outer.iter_mut().for_each(|i| *i += base);
            f.plane = None;
            f.id = None;
            p.faces.push(f);
        }
        let groups = shells(&p);
        assert_eq!(groups.len(), 2);
        assert!(pieces(&p).is_none(), "one piece and one void");
    }
}
