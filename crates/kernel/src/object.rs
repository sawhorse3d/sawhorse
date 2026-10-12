//! The solid tier: a manifold, watertight half-edge solid (ADR-0001).
//!
//! Born by extruding a sketch region (push/pull of a flat region); changed by general
//! push/pull ([`crate::push`]), drawing on faces ([`crate::faceops`]: split_face,
//! imprint) and booleans ([`crate::boolean`]). Every edit rebuilds the B-rep from plain
//! data and must validate (topology, watertight, no self-intersection) before commit.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};
use slotmap::Key;

use sawhorse_tessellate::{TessellateError, signed_area, triangulate};

use crate::boolean::{BoolOp, BooleanError, TriSolid};
use crate::brep::{Brep, PolyFace, Polys};
use crate::error::{FaceEditError, PushError};
use crate::ids::{FaceId, SketchId, VertexId};
use crate::plane::Plane;
use crate::tolerance::{MIN_EXTRUDE, in_bounds};
use crate::validate::{TopologyError, TopologyReport, Violation, validate};

/// Where a solid came from, so collapsing it can return its footprint to that sketch.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    pub sketch: SketchId,
}

#[derive(Debug, Clone)]
pub struct Object {
    pub(crate) brep: Brep,
    pub(crate) watertight: bool,
    pub(crate) source: Option<Provenance>,
    /// Tag (layer) name; `None` is untagged. Hidden tags hide the object in the view.
    pub(crate) tag: Option<String>,
    /// Display name (the Outliner, export file object names); `None` is unnamed.
    pub(crate) name: Option<String>,
    /// Material name (a key of the model's material table); `None` is the default look.
    pub(crate) material: Option<String>,
    /// Next persistent face id to hand out. Face ids start at 1 and are never reused, so an
    /// id the UI holds either names the same face (possibly reshaped) or no face at all.
    pub(crate) next_face_id: u64,
    /// Display number ("Solid 3"), handed out by the model when the object is created and
    /// never reused within a document's history. The UI names solids by it instead of by
    /// public id (which carries slot-reuse bits). Not part of the model hash: it is a pure
    /// function of the op history, like the ids.
    pub(crate) number: u64,
    /// Where it sits in the model's tree (its group or component instance), and whether it
    /// is hidden or locked (see [`crate::nodes`]).
    pub(crate) member: crate::nodes::Membership,
    /// What was said about it as a part of a cut list: grain and stock (lane 7). Carried
    /// into a component definition's template with the solid, like its material.
    pub(crate) part: crate::cutlist::PartAttrs,
    /// Paint of single faces, by persistent face id (lane 10): their own material and how
    /// a texture sits on them. Only faces with something said are listed. Pieces of a
    /// split face take their parent's paint; faces that disappear take theirs with them.
    pub(crate) paint: BTreeMap<u64, crate::looks::FacePaint>,
}

/// Give every face of `p` a persistent id: a face keeps the id it carries unless an
/// earlier face already took it (the first piece of a split keeps the id, the second
/// gets a new one); faces without one get `*next`, `*next + 1`, ... in face order.
/// Returns the lineage of the faces that lost a duplicate id, as (new id, the id it
/// carried), so paint follows a split onto both pieces.
pub fn assign_face_ids(p: &mut Polys, next: &mut u64) -> Vec<(u64, u64)> {
    let mut used = BTreeSet::new();
    let mut from: Vec<Option<u64>> = vec![None; p.faces.len()];
    for (k, f) in p.faces.iter_mut().enumerate() {
        match f.id {
            Some(id) if id != 0 && used.insert(id) => {
                *next = (*next).max(id + 1);
            }
            Some(id) if id != 0 => {
                from[k] = Some(id);
                f.id = None;
            }
            _ => f.id = None,
        }
    }
    let mut lineage = Vec::new();
    for (k, f) in p.faces.iter_mut().enumerate() {
        if f.id.is_none() {
            f.id = Some(*next);
            used.insert(*next);
            if let Some(old) = from[k] {
                lineage.push((*next, old));
            }
            *next += 1;
        }
    }
    lineage
}

#[derive(Debug, Clone, PartialEq)]
pub enum PushOutcome {
    /// The face moved.
    Moved,
    /// The face met the opposite face: the solid has zero volume and should be removed.
    /// `footprint`: the flat rings (outer, then holes) it collapsed onto, world coordinates.
    Collapsed { footprint: Vec<Vec<DVec3>> },
}

impl Object {
    /// Extrude a planar region (outer ring + hole rings, in `plane`'s 2D frame) by
    /// `distance` along the plane normal (negative goes the other way).
    ///
    /// Face order is deterministic: base (on the plane), cap (offset), then side walls ring
    /// by ring, edge by edge.
    pub fn extrude(
        plane: &Plane,
        outer: &[DVec2],
        holes: &[Vec<DVec2>],
        distance: f64,
    ) -> Result<Object, PushError> {
        if !in_bounds(distance) {
            return Err(PushError::OutOfBounds);
        }
        if distance.abs() < MIN_EXTRUDE {
            return Err(PushError::ZeroDistance);
        }
        // Normalise winding: outer counter-clockwise, holes clockwise.
        let mut rings: Vec<Vec<DVec2>> = Vec::with_capacity(1 + holes.len());
        let mut o = outer.to_vec();
        if signed_area(&o) < 0.0 {
            o.reverse();
        }
        rings.push(o);
        for h in holes {
            let mut h = h.clone();
            if signed_area(&h) > 0.0 {
                h.reverse();
            }
            rings.push(h);
        }
        let offset = plane.normal * distance;
        let mut points: Vec<DVec3> = Vec::new();
        let mut base_idx: Vec<Vec<usize>> = Vec::new();
        for r in &rings {
            base_idx.push((points.len()..points.len() + r.len()).collect());
            points.extend(r.iter().map(|&q| plane.to_world(q)));
        }
        let n_base = points.len();
        let cap_idx: Vec<Vec<usize>> = base_idx
            .iter()
            .map(|r| r.iter().map(|&i| i + n_base).collect())
            .collect();
        for i in 0..n_base {
            points.push(points[i] + offset);
        }
        if points
            .iter()
            .any(|p| !in_bounds(p.x) || !in_bounds(p.y) || !in_bounds(p.z))
        {
            return Err(PushError::OutOfBounds);
        }

        let rev = |r: &Vec<usize>| r.iter().rev().copied().collect::<Vec<_>>();
        let mut faces = Vec::new();
        // Built for distance > 0; mirrored below when negative.
        faces.push(PolyFace::new(
            rev(&base_idx[0]),
            base_idx[1..].iter().map(rev).collect(),
        ));
        faces.push(PolyFace::new(cap_idx[0].clone(), cap_idx[1..].to_vec()));
        for (b, c) in base_idx.iter().zip(&cap_idx) {
            let n = b.len();
            for k in 0..n {
                let j = (k + 1) % n;
                faces.push(PolyFace::new(vec![b[k], b[j], c[j], c[k]], vec![]));
            }
        }
        if distance < 0.0 {
            for f in &mut faces {
                f.outer.reverse();
                for h in &mut f.holes {
                    h.reverse();
                }
            }
        }
        for (k, f) in faces.iter_mut().enumerate() {
            f.id = Some(k as u64 + 1);
        }
        let brep = Brep::from_polygons(&points, &faces)?;
        let mut obj = Object {
            brep,
            watertight: false,
            source: None,
            tag: None,
            name: None,
            material: None,
            next_face_id: faces.len() as u64 + 1,
            number: 0,
            member: Default::default(),
            part: Default::default(),
            paint: Default::default(),
        };
        obj.watertight = validate(&obj.brep).map(|r| r.watertight).unwrap_or(false);
        Ok(obj)
    }

    pub fn brep(&self) -> &Brep {
        &self.brep
    }

    pub fn is_watertight(&self) -> bool {
        self.watertight
    }

    pub fn source(&self) -> Option<&Provenance> {
        self.source.as_ref()
    }

    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// Display number ("Solid 3"): stable for the object's life, never reused.
    pub fn number(&self) -> u64 {
        self.number
    }

    /// Axis-aligned bounds (min, max) of the solid's vertices, metres.
    pub fn bounds(&self) -> (DVec3, DVec3) {
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        for (_, v) in self.brep.vertices() {
            lo = lo.min(v.pos);
            hi = hi.max(v.pos);
        }
        (lo, hi)
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn material(&self) -> Option<&str> {
        self.material.as_deref()
    }

    /// Carry provenance, tag, name, material, display number and place in the tree over
    /// from the object this one replaces.
    pub(crate) fn copy_meta(&mut self, from: &Object) {
        self.source = from.source.clone();
        self.tag = from.tag.clone();
        self.name = from.name.clone();
        self.material = from.material.clone();
        self.number = from.number;
        self.member = from.member.clone();
        self.part = from.part.clone();
        // Paint this object already took from a split (its lineage) stays; the rest is the
        // original's.
        let own = std::mem::take(&mut self.paint);
        self.paint = from.paint.clone();
        self.paint.extend(own);
        self.settle_paint(&[]);
    }

    /// Faces painted (lane 10): their own material and texture position, by face id.
    pub fn face_paint(&self) -> &BTreeMap<u64, crate::looks::FacePaint> {
        &self.paint
    }

    /// After an edit: new faces that came from a painted face take its paint (`lineage`:
    /// (new id, the id it came from)), and the paint of faces that are gone goes too.
    pub(crate) fn settle_paint(&mut self, lineage: &[(u64, u64)]) {
        if self.paint.is_empty() {
            return;
        }
        for &(new, old) in lineage {
            if let Some(p) = self.paint.get(&old).cloned() {
                self.paint.insert(new, p);
            }
        }
        let live: BTreeSet<u64> = self.brep.persistent_face_ids().into_iter().collect();
        self.paint.retain(|id, _| live.contains(id));
    }

    /// Painted faces on `plane` and the paint of the first: what a stroke or an imprint on
    /// that plane passes to the faces it makes.
    fn paint_on_plane(&self, plane: &Plane) -> Option<crate::looks::FacePaint> {
        self.brep
            .faces()
            .filter(|(_, f)| {
                f.plane.normal.dot(plane.normal) > 1.0 - 1e-9
                    && plane.signed_distance(f.plane.origin).abs() <= crate::tolerance::PLANARITY
            })
            .find_map(|(_, f)| self.paint.get(&f.id).cloned())
    }

    /// Faces an edit made on `plane` (not in `before`) take `paint`.
    fn give_paint_on_plane(
        &mut self,
        before: &BTreeSet<u64>,
        plane: &Plane,
        paint: Option<crate::looks::FacePaint>,
    ) {
        let Some(p) = paint else { return };
        let fresh: Vec<u64> = self
            .brep
            .faces()
            .filter(|(_, f)| {
                !before.contains(&f.id)
                    && f.plane.normal.dot(plane.normal) > 1.0 - 1e-9
                    && plane.signed_distance(f.plane.origin).abs() <= crate::tolerance::PLANARITY
            })
            .map(|(_, f)| f.id)
            .collect();
        for id in fresh {
            self.paint.entry(id).or_insert_with(|| p.clone());
        }
    }

    /// Grain and stock said about this solid as a part (lane 7).
    pub fn part(&self) -> &crate::cutlist::PartAttrs {
        &self.part
    }

    /// Its group or component instance, slot and hidden / locked flags.
    pub fn membership(&self) -> &crate::nodes::Membership {
        &self.member
    }

    /// Faces in deterministic order (index = the face ordinal).
    pub fn face_ids(&self) -> Vec<FaceId> {
        self.brep.face_ids()
    }

    /// Ordinal of the face with persistent id `id` (what ops and the UI address).
    pub fn face_ordinal(&self, id: u64) -> Option<usize> {
        self.brep.face_ordinal(id)
    }

    /// Persistent id of the face at ordinal `ordinal`.
    pub fn face_id_at(&self, ordinal: usize) -> Option<u64> {
        self.brep.faces.values().nth(ordinal).map(|f| f.id)
    }

    /// The next persistent face id this object will hand out.
    pub fn next_face_id(&self) -> u64 {
        self.next_face_id
    }

    /// Rebuild from plain data, giving new faces fresh persistent ids (never reusing one
    /// this object handed out). Keeps provenance and tag. Not validated.
    pub(crate) fn rebuilt(&self, mut p: Polys) -> Result<Object, crate::error::BuildError> {
        let mut next = self.next_face_id;
        let lineage = assign_face_ids(&mut p, &mut next);
        let brep = Brep::build(&p)?;
        let mut o = Object::from_brep(brep);
        o.next_face_id = o.next_face_id.max(next);
        o.copy_meta(self);
        o.settle_paint(&lineage);
        Ok(o)
    }

    /// Validate topology plus the object rules: watertight, and the stored flag agrees.
    pub fn validate(&self) -> Result<TopologyReport, TopologyError> {
        let report = validate(&self.brep)?;
        let mut v = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for (_, f) in self.brep.faces() {
            if f.id == 0 || f.id >= self.next_face_id || !ids.insert(f.id) {
                v.push(Violation::DanglingKey(
                    "face ids must be non-zero, unique and below the object's counter",
                ));
                break;
            }
        }
        if !report.watertight || !self.watertight {
            v.push(Violation::DanglingKey("object is not watertight"));
        }
        if report.shells.iter().any(|s| !s.closed) {
            v.push(Violation::DanglingKey("object has an open shell"));
        }
        if let Some(hit) = crate::intersect::self_intersection(&self.brep) {
            v.push(Violation::SelfIntersectingSolid(hit));
        }
        // Every face can be locally consistent while the whole solid is inside out (a face
        // pushed backwards through itself): the enclosed volume must be positive.
        match self.tri_solid() {
            Ok(t) => {
                let volume = t.volume();
                if volume.is_nan() || volume <= 0.0 {
                    v.push(Violation::NonPositiveVolume(volume));
                }
            }
            Err(e) => v.push(Violation::Untessellatable(e.to_string())),
        }
        if v.is_empty() {
            Ok(report)
        } else {
            Err(TopologyError {
                violations: v,
                report,
            })
        }
    }

    /// Push/pull face `face` (ordinal) by `distance` metres along its outward normal
    /// (negative pushes inward). Any planar face of a watertight solid; see [`crate::push`]
    /// for the collapse / topological / boolean paths. On `Moved` the solid is replaced;
    /// on `Collapsed` it is left as it was (the caller removes it). Face ids survive: the
    /// pushed face keeps its id on both paths (on the boolean path the swept prism's cap
    /// inherits it), faces the push reshapes keep theirs, new walls get fresh ones.
    pub fn push_pull(&mut self, face: usize, distance: f64) -> Result<PushOutcome, PushError> {
        let (outcome, next) = crate::push::push_pull(self, face, distance)?;
        if let Some(mut next) = next {
            next.copy_meta(self);
            *self = next;
        }
        Ok(outcome)
    }

    /// Push/pull leaving the face's old boundary as a seam (Ctrl in the UI; outward pulls
    /// only, see [`crate::push::push_pull_with`]).
    pub fn push_pull_new_face(
        &mut self,
        face_id: u64,
        distance: f64,
    ) -> Result<PushOutcome, PushError> {
        let fi = self
            .face_ordinal(face_id)
            .ok_or(PushError::NoSuchFace(face_id))?;
        let (outcome, next) = crate::push::push_pull_with(self, fi, distance, true)?;
        if let Some(mut next) = next {
            next.copy_meta(self);
            *self = next;
        }
        Ok(outcome)
    }

    /// Push/pull the face with persistent id `face_id` (see [`Object::push_pull`]).
    pub fn push_pull_id(&mut self, face_id: u64, distance: f64) -> Result<PushOutcome, PushError> {
        let fi = self
            .face_ordinal(face_id)
            .ok_or(PushError::NoSuchFace(face_id))?;
        self.push_pull(fi, distance)
    }

    /// A solid from an already-built B-rep (watertight flag from the validator). Faces
    /// without an id (a bare B-rep) get fresh ones after the largest id present.
    pub(crate) fn from_brep(mut brep: Brep) -> Object {
        let watertight = validate(&brep).map(|r| r.watertight).unwrap_or(false);
        let mut next = brep.faces.values().map(|f| f.id + 1).max().unwrap_or(1);
        for f in brep.faces.values_mut() {
            if f.id == 0 {
                f.id = next;
                next += 1;
            }
        }
        Object {
            brep,
            watertight,
            source: None,
            tag: None,
            name: None,
            material: None,
            next_face_id: next,
            number: 0,
            member: Default::default(),
            part: Default::default(),
            paint: Default::default(),
        }
    }

    /// `self op other` through the boolean engine, re-polygonised (see
    /// [`crate::boolean::combine`]). `Ok(None)`: the result is empty.
    pub fn combine(&self, other: &Object, op: BoolOp) -> Result<Option<Object>, BooleanError> {
        let engine = crate::boolean::default_engine()?;
        let res = crate::boolean::combine(
            &self.brep.to_polys(),
            &other.brep.to_polys(),
            op,
            engine.as_ref(),
        )?;
        let Some(p) = res else { return Ok(None) };
        let o = self
            .rebuilt(p)
            .map_err(|e| BooleanError::InvalidResult(e.to_string()))?;
        o.validate()
            .map_err(|e| BooleanError::InvalidResult(e.to_string()))?;
        Ok(Some(o))
    }

    /// Total face area in square metres.
    pub fn surface_area(&self) -> f64 {
        self.brep
            .faces()
            .map(|(_, f)| {
                f.loops()
                    .map(|l| {
                        let pts: Vec<DVec2> = self
                            .brep
                            .loop_points(l)
                            .iter()
                            .map(|&p| f.plane.to_local(p))
                            .collect();
                        signed_area(&pts)
                    })
                    .sum::<f64>()
            })
            .sum()
    }

    /// Split face `face` (ordinal) along `path` (world points on the face's plane): ends on
    /// the boundary, interior strictly inside. Returns the two faces' persistent ids (the
    /// first piece keeps the face's id, the second gets a new one). See
    /// [`crate::faceops::split_face`].
    pub fn split_face(&mut self, face: usize, path: &[DVec3]) -> Result<[u64; 2], FaceEditError> {
        let mut p = self.brep.to_polys();
        if face >= p.faces.len() {
            return Err(FaceEditError::NoSuchFace(face as u64));
        }
        let (a, b) = crate::faceops::split_face(&mut p, face, path)?;
        let lineage = assign_face_ids(&mut p, &mut self.next_face_id);
        let ids = [p.faces[a].id, p.faces[b].id].map(|i| i.unwrap_or(0));
        self.brep = Brep::build(&p)?;
        self.brep.debug_validate("split_face");
        self.refuse_self_contact()?;
        self.settle_paint(&lineage);
        Ok(ids)
    }

    /// Imprint the closed loop `points` (strictly inside face `face`) as a coplanar
    /// sub-face; the face gains the twin hole. Returns the sub-face's persistent id (new).
    /// See [`crate::faceops::imprint`].
    pub fn imprint(&mut self, face: usize, points: &[DVec3]) -> Result<u64, FaceEditError> {
        let mut p = self.brep.to_polys();
        if face >= p.faces.len() {
            return Err(FaceEditError::NoSuchFace(face as u64));
        }
        let before: BTreeSet<u64> = self.brep.persistent_face_ids().into_iter().collect();
        let plane = p.plane(face);
        let host = plane.and_then(|pl| self.paint_on_plane(&pl));
        let sub = crate::faceops::imprint(&mut p, face, points)?;
        let lineage = assign_face_ids(&mut p, &mut self.next_face_id);
        let id = p.faces[sub].id.unwrap_or(0);
        self.brep = Brep::build(&p)?;
        self.brep.debug_validate("imprint");
        self.refuse_self_contact()?;
        self.settle_paint(&lineage);
        if let Some(pl) = plane {
            self.give_paint_on_plane(&before, &pl, host);
        }
        Ok(id)
    }

    /// Draw stroke `points` on the plane of face `face` (ordinal): cuts where it runs from
    /// edge to edge, an imprint when it closes inside one face (see [`crate::draw`]).
    /// Returns the persistent ids of the faces it cut or made, sorted, and for a closed
    /// stroke the id of the face it now encloses (the strip of a dado, an imprint).
    pub fn draw(
        &mut self,
        face: usize,
        points: &[DVec3],
        closed: bool,
    ) -> Result<(Vec<u64>, Option<u64>), FaceEditError> {
        let mut p = self.brep.to_polys();
        if face >= p.faces.len() {
            return Err(FaceEditError::NoSuchFace(face as u64));
        }
        let plane = p.plane(face).ok_or(FaceEditError::Degenerate)?;
        let before: BTreeSet<u64> = self.brep.persistent_face_ids().into_iter().collect();
        // A painted face on the stroke's plane passes its paint to the pieces made there.
        let host = self.paint_on_plane(&plane);
        let touched = crate::draw::draw_on_faces(&mut p, face, points, closed)?;
        let inside = if closed {
            let ring: Vec<DVec2> = points.iter().map(|&q| plane.to_local(q)).collect();
            sawhorse_tessellate::interior_point(&ring, &[])
                .and_then(|q| crate::draw::face_containing(&p, &plane, q))
        } else {
            None
        };
        let lineage = assign_face_ids(&mut p, &mut self.next_face_id);
        let mut ids: Vec<u64> = touched
            .iter()
            .filter_map(|&i| p.faces.get(i).and_then(|f| f.id))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let inside = inside.and_then(|i| p.faces.get(i).and_then(|f| f.id));
        self.brep = Brep::build(&p)?;
        self.brep.debug_validate("draw");
        self.refuse_self_contact()?;
        self.settle_paint(&lineage);
        self.give_paint_on_plane(&before, &plane, host);
        Ok((ids, inside))
    }

    /// Erase the edge between `a` and `b` (a line drawn on a face): its two coplanar faces
    /// merge. Returns the merged face's persistent id. See [`crate::faceops::erase_edge`].
    pub fn erase_edge(&mut self, a: DVec3, b: DVec3) -> Result<u64, FaceEditError> {
        let mut p = self.brep.to_polys();
        let fi = crate::faceops::erase_edge(&mut p, a, b)?;
        self.finish_face_edit(p, fi, "erase_edge")
    }

    /// Erase face `face` (ordinal) into a coplanar neighbour across drawn lines. Returns
    /// the absorbing face's persistent id. See [`crate::faceops::erase_face`].
    pub fn erase_face(&mut self, face: usize) -> Result<u64, FaceEditError> {
        let mut p = self.brep.to_polys();
        let fi = crate::faceops::erase_face(&mut p, face)?;
        self.finish_face_edit(p, fi, "erase_face")
    }

    fn finish_face_edit(
        &mut self,
        mut p: Polys,
        fi: usize,
        what: &str,
    ) -> Result<u64, FaceEditError> {
        let lineage = assign_face_ids(&mut p, &mut self.next_face_id);
        let id = p.faces.get(fi).and_then(|f| f.id).unwrap_or(0);
        self.brep = Brep::build(&p)?;
        self.brep.debug_validate(what);
        self.refuse_self_contact()?;
        self.settle_paint(&lineage);
        Ok(id)
    }

    /// A face edit that brings a new corner or edge within the merge distance of another
    /// face (a loop drawn on a sliver a few micrometres thick) would leave the solid
    /// touching itself: refused, typed, instead of reaching the validator as a bug.
    fn refuse_self_contact(&self) -> Result<(), FaceEditError> {
        match crate::intersect::self_intersection(&self.brep) {
            Some(_) => Err(FaceEditError::TooClose),
            None => Ok(()),
        }
    }

    /// Indexed triangle solid (shared vertices in slotmap order, face ordinal per triangle),
    /// the form booleans and exporters consume.
    pub fn tri_solid(&self) -> Result<TriSolid, TessellateError> {
        let b = &self.brep;
        let vindex: std::collections::BTreeMap<VertexId, u32> = b
            .vertices
            .keys()
            .enumerate()
            .map(|(i, k)| (k, i as u32))
            .collect();
        let positions: Vec<DVec3> = b.vertices.values().map(|v| v.pos).collect();
        let mut triangles = Vec::new();
        let mut face_ids = Vec::new();
        for (fi, (_, f)) in b.faces.iter().enumerate() {
            let loops: Vec<Vec<VertexId>> = f.loops().map(|l| b.loop_vertices(l)).collect();
            let flat: Vec<VertexId> = loops.iter().flatten().copied().collect();
            let ring2 = |l: &Vec<VertexId>| {
                l.iter()
                    .map(|v| f.plane.to_local(b.vertices[*v].pos))
                    .collect::<Vec<_>>()
            };
            let outer = ring2(&loops[0]);
            let holes: Vec<Vec<DVec2>> = loops[1..].iter().map(ring2).collect();
            for t in triangulate(&outer, &holes)? {
                triangles.push(t.map(|i| vindex[&flat[i as usize]]));
                face_ids.push(fi as u32);
            }
        }
        Ok(TriSolid {
            positions,
            triangles,
            face_ids,
        })
    }

    /// Enclosed volume in cubic metres.
    pub fn volume(&self) -> f64 {
        self.tri_solid().map(|s| s.volume()).unwrap_or(0.0)
    }

    /// True when both objects have the same topology (vertex/face order) and every vertex
    /// is within `tol`. Used by the inverse-operation invariants.
    pub fn same_shape(&self, other: &Object, tol: f64) -> bool {
        let (a, b) = (&self.brep, &other.brep);
        if a.vertex_count() != b.vertex_count()
            || a.face_count() != b.face_count()
            || a.edge_count() != b.edge_count()
        {
            return false;
        }
        let close = a
            .vertices
            .values()
            .zip(b.vertices.values())
            .all(|(p, q)| p.pos.distance(q.pos) <= tol);
        let topo = a.faces.values().zip(b.faces.values()).all(|(f, g)| {
            let fl: Vec<Vec<VertexId>> = f.loops().map(|l| a.loop_vertices(l)).collect();
            let gl: Vec<Vec<VertexId>> = g.loops().map(|l| b.loop_vertices(l)).collect();
            fl.len() == gl.len()
                && fl.iter().zip(&gl).all(|(x, y)| {
                    x.len() == y.len()
                        && x.iter()
                            .zip(y)
                            .all(|(p, q)| p.data().as_ffi() == q.data().as_ffi())
                })
        });
        close && topo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> Vec<DVec2> {
        vec![
            DVec2::ZERO,
            DVec2::new(w, 0.0),
            DVec2::new(w, h),
            DVec2::new(0.0, h),
        ]
    }

    #[test]
    fn extrude_makes_a_watertight_box() {
        let o = Object::extrude(&Plane::GROUND, &rect(2.0, 3.0), &[], 4.0).unwrap();
        let r = o.validate().unwrap();
        assert!(r.watertight);
        assert_eq!((r.vertices, r.edges, r.faces), (8, 12, 6));
        assert_eq!(r.shells[0].genus, Some(0));
        assert!((o.volume() - 24.0).abs() < 1e-9);
    }

    #[test]
    fn negative_extrude_points_down_and_stays_outward() {
        let o = Object::extrude(&Plane::GROUND, &rect(1.0, 1.0), &[], -0.5).unwrap();
        o.validate().unwrap();
        assert!(
            (o.volume() - 0.5).abs() < 1e-12,
            "outward orientation gives positive volume"
        );
        assert!(o.brep.vertices().all(|(_, v)| v.pos.z <= 1e-12));
    }

    #[test]
    fn extrude_with_hole_is_genus_one() {
        let mut hole = vec![
            DVec2::ONE,
            DVec2::new(3.0, 1.0),
            DVec2::new(3.0, 3.0),
            DVec2::new(1.0, 3.0),
        ];
        hole.reverse();
        let o = Object::extrude(&Plane::GROUND, &rect(4.0, 4.0), &[hole], 1.0).unwrap();
        let r = o.validate().unwrap();
        assert_eq!(r.holes, 2);
        assert_eq!(r.shells[0].euler, 0);
        assert_eq!(r.shells[0].genus, Some(1));
        assert!((o.volume() - 12.0).abs() < 1e-9);
    }

    #[test]
    fn typed_refusals() {
        assert_eq!(
            Object::extrude(&Plane::GROUND, &rect(1.0, 1.0), &[], 1e-7).unwrap_err(),
            PushError::ZeroDistance
        );
        assert_eq!(
            Object::extrude(&Plane::GROUND, &rect(1.0, 1.0), &[], f64::INFINITY).unwrap_err(),
            PushError::OutOfBounds
        );
    }
}
