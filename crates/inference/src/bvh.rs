//! A small bounding-volume hierarchy over axis-aligned boxes (ADR-0001: our own BVH, no
//! `bvh` crate). Built by median split on the longest axis; queried with a box predicate,
//! so the same tree answers "what does this ray hit" and "what passes within N pixels of
//! this ray" (the predicate inflates each node by the pixel size at its far corner).

use crate::geom::Aabb;

const LEAF: usize = 4;

#[derive(Debug, Clone)]
struct Node {
    bb: Aabb,
    /// Children (internal node) or the item range `start..start + count` (leaf).
    left: u32,
    right: u32,
    start: u32,
    count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    items: Vec<u32>,
    boxes: Vec<Aabb>,
}

impl Bvh {
    /// Build over `boxes` (item `i` has box `boxes[i]`). Deterministic.
    pub fn build(boxes: &[Aabb]) -> Bvh {
        let mut b = Bvh {
            nodes: Vec::new(),
            items: (0..boxes.len() as u32).collect(),
            boxes: boxes.to_vec(),
        };
        if !boxes.is_empty() {
            let n = boxes.len();
            b.split(boxes, 0, n);
        }
        b
    }

    fn split(&mut self, boxes: &[Aabb], start: usize, end: usize) -> u32 {
        let bb = self.items[start..end]
            .iter()
            .fold(Aabb::EMPTY, |acc, &i| acc.union(boxes[i as usize]));
        let id = self.nodes.len() as u32;
        self.nodes.push(Node {
            bb,
            left: 0,
            right: 0,
            start: start as u32,
            count: (end - start) as u32,
        });
        if end - start <= LEAF {
            return id;
        }
        let ext = bb.max - bb.min;
        let axis = if ext.x >= ext.y && ext.x >= ext.z {
            0
        } else if ext.y >= ext.z {
            1
        } else {
            2
        };
        self.items[start..end].sort_by(|&a, &b| {
            boxes[a as usize].centre()[axis]
                .total_cmp(&boxes[b as usize].centre()[axis])
                .then(a.cmp(&b))
        });
        let mid = (start + end) / 2;
        let l = self.split(boxes, start, mid);
        let r = self.split(boxes, mid, end);
        let node = &mut self.nodes[id as usize];
        node.left = l;
        node.right = r;
        node.count = 0;
        id
    }

    /// Visit every item whose own box and enclosing node boxes pass `accept` (in ascending
    /// item order, so results are deterministic).
    pub fn query(&self, accept: impl Fn(&Aabb) -> bool, mut visit: impl FnMut(usize)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut found: Vec<u32> = Vec::new();
        let mut stack = vec![0u32];
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n as usize];
            if !accept(&node.bb) {
                continue;
            }
            if node.count > 0 {
                let s = node.start as usize;
                found.extend(
                    self.items[s..s + node.count as usize]
                        .iter()
                        .filter(|&&i| accept(&self.boxes[i as usize])),
                );
            } else {
                stack.push(node.right);
                stack.push(node.left);
            }
        }
        found.sort_unstable();
        for i in found {
            visit(i as usize);
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Ray;
    use glam::DVec3;

    #[test]
    fn finds_exactly_the_boxes_a_ray_crosses() {
        // A 10 x 10 grid of unit boxes on z = 0, one per cell.
        let boxes: Vec<Aabb> = (0..100)
            .map(|i| {
                let (x, y) = ((i % 10) as f64, (i / 10) as f64);
                Aabb::of(&[DVec3::new(x, y, 0.0), DVec3::new(x + 0.9, y + 0.9, 0.1)])
            })
            .collect();
        let bvh = Bvh::build(&boxes);
        assert_eq!(bvh.len(), 100);
        let ray = Ray::new(DVec3::new(3.5, 7.5, 5.0), DVec3::NEG_Z).unwrap();
        let mut hits = Vec::new();
        bvh.query(|b| b.hit_by(&ray), |i| hits.push(i));
        assert_eq!(hits, vec![73]);
        // Brute force agrees for a slanted ray.
        let ray = Ray::new(DVec3::new(-1.0, 0.45, 0.05), DVec3::new(1.0, 0.3, 0.0)).unwrap();
        let mut got = Vec::new();
        bvh.query(|b| b.hit_by(&ray), |i| got.push(i));
        let want: Vec<usize> = (0..100).filter(|&i| boxes[i].hit_by(&ray)).collect();
        assert_eq!(got, want);
        assert!(!got.is_empty());
    }
}
