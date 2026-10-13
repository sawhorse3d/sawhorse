//! Shared invariants for the op-sequence fuzzer, the repro replays and cargo-fuzz.
//!
//! [`check_sequence`] runs a sequence of ops from an empty model and checks, after every
//! op:
//! - an accepted op leaves a model that validates (topology, watertight solids, sketch
//!   graph), whose solids have positive volume, and that tessellates for rendering;
//! - a refused op returns a typed refusal (never a validator "bug" error) and leaves the
//!   model bit-for-bit unchanged (same hash);
//! - nothing panics (in debug builds the per-mutation validator panics on corruption);
//! - (lane 5) the tree validates too: every component instance holds exactly its
//!   definition, each solid its definition's solid under the instance's placement;
//! - (lane 7) the cut list works out without panicking and agrees with a brute-force
//!   count of the world ([`check_cut_list`]);
//! - (lane 9) the nesting works out with both algorithms, deterministically, and keeps
//!   every layout invariant ([`check_nesting`]);
//!
//! and finally that replaying the same sequence again gives the same hash (determinism).
//! Failing sequences are written as JSON by [`write_repro`] and replayed forever after by
//! `tests/repro.rs`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::document::Document;
use crate::mesh::view_mesh;
use crate::model::{Model, Op};

/// What a clean run looked like (for fuzzer statistics).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunSummary {
    pub accepted: usize,
    pub refused: usize,
    pub objects: usize,
}

fn panic_message(p: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic".to_string()
    }
}

/// Invariants of any committed model.
pub fn check_model(m: &Model) -> Result<(), String> {
    m.validate().map_err(|e| format!("validate: {e}"))?;
    for (i, o) in m.objects() {
        if !o.is_watertight() {
            return Err(format!("object {i} is not watertight"));
        }
        let v = o.volume();
        if v.is_nan() || v <= 0.0 {
            return Err(format!("object {i} has non-positive volume {v}"));
        }
        // Severance by connectivity (lane 4): a solid that came apart is split into one
        // object per piece, so every committed object is exactly one piece.
        let pieces = o.piece_count();
        if pieces != 1 {
            return Err(format!(
                "object {i} has {pieces} pieces (it should have been severed)"
            ));
        }
    }
    let mesh = view_mesh(m).map_err(|e| format!("view mesh: {e}"))?;
    if mesh.indices.len() % 3 != 0 || mesh.face_ids.len() * 3 != mesh.indices.len() {
        return Err("render mesh index/face-id mismatch".into());
    }
    check_edge_flags(m, &mesh)?;
    check_looks(m, &mesh)?;
    check_cut_list(m)?;
    check_nesting(m)?;
    check_sections(m)
}

/// Every section plane's cut (lane 12) of any committed model: it never fails, it is
/// deterministic, its caps are closed simple rings with the material on the left (outers
/// counter-clockwise, holes clockwise, holes inside their outer, no point twice), its area
/// is the caps' sum, its outline has an edge per ring edge (less those lying on another
/// plane's cut), each half a plane and its flip keep is a closed surface (lane 12c: its
/// vector area cancels), and the two halves add up to the solids it reaches.
pub fn check_sections(m: &Model) -> Result<(), String> {
    use glam::DVec2;
    for (id, s) in m.sections() {
        let cut = m
            .section_cut(id)
            .map_err(|e| format!("section {id}: the cut failed: {e}"))?;
        let again = m.section_cut(id).map_err(|e| e.to_string())?;
        if again != cut {
            return Err(format!("section {id}: the cut is not deterministic"));
        }
        let mut area = 0.0;
        for c in &cut.caps {
            // Simple rings (lane 12c): a ring through one point twice is two rings chained
            // through a pinch, which the cap must have split.
            for r in std::iter::once(&c.outer).chain(&c.holes) {
                let mut seen = std::collections::BTreeSet::new();
                if !r
                    .iter()
                    .all(|p| seen.insert((p[0].to_bits(), p[1].to_bits())))
                {
                    return Err(format!("section {id}: a cap ring passes a point twice"));
                }
            }
            let outer: Vec<DVec2> = c.outer.iter().map(|p| DVec2::from_array(*p)).collect();
            if outer.len() < 3 || !outer.iter().all(|p| p.is_finite()) {
                return Err(format!("section {id}: a cap's outer ring is degenerate"));
            }
            let a = sawhorse_tessellate::signed_area(&outer);
            if a <= 0.0 {
                return Err(format!(
                    "section {id}: a cap's outer ring is clockwise ({a})"
                ));
            }
            let mut sum = a;
            for h in &c.holes {
                let ring: Vec<DVec2> = h.iter().map(|p| DVec2::from_array(*p)).collect();
                let ha = sawhorse_tessellate::signed_area(&ring);
                if ha >= 0.0 {
                    return Err(format!("section {id}: a cap's hole is counter-clockwise"));
                }
                sum += ha;
            }
            if (sum - c.area).abs() > 1e-12 * (1.0 + sum.abs()) || c.area <= 0.0 {
                return Err(format!(
                    "section {id}: a cap's area {} is not its rings' {sum}",
                    c.area
                ));
            }
            area += c.area;
        }
        if (area - cut.area).abs() > 1e-12 * (1.0 + area.abs()) {
            return Err(format!("section {id}: the total area is not the caps' sum"));
        }
        let ring_edges: usize = cut
            .caps
            .iter()
            .map(|c| c.outer.len() + c.holes.iter().map(Vec::len).sum::<usize>())
            .sum();
        if cut.outline.len() > ring_edges {
            return Err(format!("section {id}: more outline edges than ring edges"));
        }
        // The two halves: kept(plane) + kept(flipped) = the volume of what it reaches.
        let mut flipped = m.clone();
        let k = crate::ids::from_public::<crate::ids::SectionId>(id);
        flipped.sections[k].normal = -s.normal;
        let (pa, pb) = (
            m.kept_part(id).map_err(|e| e.to_string())?,
            flipped.kept_part(id).map_err(|e| e.to_string())?,
        );
        let (a, b) = (pa.volume, pb.volume);
        let reached_objects: Vec<_> = m
            .objects()
            .into_iter()
            .filter(|(o, x)| {
                m.is_visible(*o)
                    && s.parent
                        .is_none_or(|p| m.path_of(x.member.parent).contains(&p))
            })
            .collect();
        let reached: f64 = reached_objects.iter().map(|(_, x)| x.volume()).sum();
        // The kept volume integrates every face over its STORED plane (the divergence
        // theorem on the face's 2D polygon), while an object's volume sums the triangles of
        // its tessellation through the actual vertices. A face's vertices may sit up to
        // PLANARITY off its stored plane, so the two measures of the same solid differ by up
        // to that times the face's area (fuzz seed 20261802, 2026-10-12: a copy squashed to a
        // quarter and stretched to 12 m had faces 5e-7 off their planes, and the halves made
        // the plane-integrated whole exactly but missed the tessellated one by 3.4 ppm). So
        // the halves are held to the whole measured their own way, and that whole is
        // held to the tessellated volume within the planarity the validator allows.
        let mut area = 0.0;
        let mut by_planes = 0.0;
        // The same two measures over the objects that tessellate as closed solids (an open
        // mesh has no tessellated volume to hold its plane integral to).
        let (mut solid_area, mut solid_planes, mut solid_tess) = (0.0, 0.0, 0.0);
        for (_, x) in &reached_objects {
            let b = x.brep();
            let (area0, planes0) = (area, by_planes);
            let pos: std::collections::BTreeMap<_, _> =
                b.vertices().map(|(k, v)| (k, v.pos)).collect();
            for (_, f) in b.faces() {
                let mut signed = 0.0;
                for l in f.loops() {
                    let ring: Vec<_> = b
                        .loop_vertices(l)
                        .iter()
                        .map(|v| f.plane.to_local(pos[v]))
                        .collect();
                    signed += sawhorse_tessellate::signed_area(&ring);
                }
                area += signed.abs();
                by_planes += f.plane.normal.dot(f.plane.origin) * signed / 3.0;
            }
            if let Ok(t) = x.tri_solid() {
                solid_area += area - area0;
                solid_planes += by_planes - planes0;
                solid_tess += t.volume();
            }
        }
        // Watertight: each kept half (its faces clipped to the kept side, and its cap) is
        // a closed surface, so its vector area cancels (see KeptPart). A cap that is
        // missing, folded or turned the wrong way leaves its whole area here. Faces are
        // planar only to within PLANARITY, so where a face meets the plane in a slightly bent
        // line its kept part and the cap may join along chords up to its bend apart: allow
        // the farthest any corner sits off its face's plane times the cut's length.
        let bend = reached_objects
            .iter()
            .flat_map(|(_, x)| {
                let b = x.brep();
                b.faces()
                    .flat_map(move |(_, f)| {
                        f.loops()
                            .flat_map(move |l| b.loop_points(l))
                            .map(move |p| f.plane.signed_distance(p).abs())
                    })
                    .collect::<Vec<f64>>()
            })
            .fold(0.0, f64::max);
        let cut_length: f64 = cut
            .caps
            .iter()
            .flat_map(|c| std::iter::once(&c.outer).chain(&c.holes))
            .map(|r| {
                (0..r.len())
                    .map(|i| {
                        let (a, b) = (r[i], r[(i + 1) % r.len()]);
                        (a[0] - b[0]).hypot(a[1] - b[1])
                    })
                    .sum::<f64>()
            })
            .sum();
        for (half, p) in [("kept", &pa), ("cut-away", &pb)] {
            let open = p.vector_area.length();
            if open > 1e-9 * (1.0 + p.area) + 2.0 * bend * cut_length {
                return Err(format!(
                    "section {id}: the {half} half is not closed (vector area {open} of {} m2; faces bend {bend}, cut {cut_length} m long)",
                    p.area
                ));
            }
        }
        // A face lying in the plane to within the merge distance but not exactly (a tilt
        // of a micrometre) is kept by the tie rule while the cap is exactly in the plane:
        // the two then differ by up to its area times that distance. So a half may come out
        // a little below zero, never by more than the solids' area times POINT_MERGE.
        let slack = crate::tolerance::POINT_MERGE * area + 1e-12;
        if (a + b - by_planes).abs() > 1e-6 * (1.0 + by_planes.abs()) || a < -slack || b < -slack {
            return Err(format!(
                "section {id}: the halves kept {a} + {b} do not make the whole {by_planes} (tessellated {reached})"
            ));
        }
        if (solid_planes - solid_tess).abs()
            > 1e-6 * (1.0 + solid_tess) + crate::tolerance::PLANARITY * solid_area
        {
            return Err(format!(
                "section {id}: the solids it reaches measure {solid_planes} over their faces' planes but {solid_tess} tessellated"
            ));
        }
    }
    Ok(())
}

/// Soft, smooth and hidden edges (lane 15) of any committed model: every flag is 1 to 7
/// and sits on an edge that is there (a pair of faces that meet, a mesh's vertex pair); the
/// view carries one flag per edge segment and, only when an edge is smooth, a unit normal
/// per render vertex.
pub fn check_edge_flags(m: &Model, mesh: &crate::mesh::ViewMesh) -> Result<(), String> {
    use crate::edges::{ALL, SMOOTH};
    for (i, o) in m.objects() {
        let live: std::collections::BTreeSet<(u64, u64)> = o
            .brep()
            .edges()
            .filter_map(|(e, _)| o.edge_key(e))
            .collect();
        for (k, &f) in o.edge_flags() {
            if f == 0 || f & !ALL != 0 {
                return Err(format!("object {i}: edge flags {f} on {k:?}"));
            }
            if !live.contains(k) {
                return Err(format!(
                    "object {i}: a flag on faces {k:?} that do not meet"
                ));
            }
        }
    }
    for (i, lm) in m.meshes() {
        let mut live: std::collections::BTreeSet<(u32, u32)> =
            lm.edge_faces().into_keys().collect();
        live.extend(lm.lines.iter().map(|l| crate::edges::pair(l[0], l[1])));
        for (k, &f) in &lm.edge_flags {
            if f == 0 || f & !ALL != 0 || !live.contains(k) {
                return Err(format!("mesh {i}: edge flags {f} on {k:?}"));
            }
        }
    }
    if mesh.edge_flags.len() != mesh.edge_owner.len() {
        return Err("view mesh: not one edge flag per edge segment".into());
    }
    let smooth = mesh.edge_flags.iter().any(|f| f & SMOOTH != 0);
    if mesh.normals.is_empty() == smooth {
        return Err(format!(
            "view mesh: normals {} but smooth edges {smooth}",
            mesh.normals.len()
        ));
    }
    if !mesh.normals.is_empty() {
        if mesh.normals.len() != mesh.positions.len() {
            return Err("view mesh: not one normal per render vertex".into());
        }
        for n in mesh.normals.chunks(3) {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if !(0.999..=1.001).contains(&l) {
                return Err(format!("view mesh: a normal of length {l}"));
            }
        }
    }
    Ok(())
}

/// The nesting (lane 9) of any committed model: with both algorithms, everything and the
/// drawn pieces only, it never panics, it is deterministic, and it keeps every invariant
/// of [`check_nesting_of`].
pub fn check_nesting(m: &Model) -> Result<(), String> {
    use crate::cutlist::CutOptions;
    use crate::nesting::NestOptions;
    use crate::stock::NestAlgorithm;
    let runs = [
        (false, Some(NestAlgorithm::Guillotine)),
        (false, Some(NestAlgorithm::Maxrects)),
        (true, None),
    ];
    for (visible_only, algorithm) in runs {
        let opts = NestOptions {
            visible_only,
            algorithm,
        };
        let n = catch_unwind(AssertUnwindSafe(|| m.nest(&opts)))
            .map_err(|p| format!("nesting panicked: {}", panic_message(p)))?;
        if m.nest(&opts).hash != n.hash {
            return Err("nesting: not deterministic".into());
        }
        let list = m.cut_list(&CutOptions {
            visible_only,
            ..CutOptions::default()
        });
        check_nesting_of(&list, &n).map_err(|e| format!("nesting ({opts:?}): {e}"))?;
    }
    Ok(())
}

/// Every invariant of a nesting of `list`: each counted piece is placed or reported exactly
/// once (pieces in = placed + unplaced), as its own item; a placement is the part's width
/// and length, turned only when the grain allows, inside the usable area of its stock, of
/// a thickness the stock covers; no two placements overlap and every two are a kerf apart;
/// no cut crosses a part; areas, efficiency and totals add up; a guillotine or board layout
/// comes apart in straight cuts; a part reported too large or grain-bound really is.
pub fn check_nesting_of(
    list: &crate::cutlist::CutList,
    n: &crate::nesting::Nesting,
) -> Result<(), String> {
    use crate::cutlist::Grain;
    use crate::nesting::{CutDir, EPS, UnplacedReason, effective_trim};
    use crate::stock::{
        NestAlgorithm, SHEET_THICKNESS_SLACK, STOCK_EPSILON, StockGrain, StockKind,
    };
    use std::collections::BTreeSet;
    let counted = |row: &crate::cutlist::CutRow| -> Vec<crate::nodes::Item> {
        row.occurrences
            .iter()
            .filter(|o| !list.visible_only || o.visible)
            .map(|o| o.item)
            .collect()
    };
    let mut want = BTreeSet::new();
    for (ri, row) in list.rows.iter().enumerate() {
        // A row dropped from the list is never nested (lane 15).
        if !row.included {
            continue;
        }
        for c in 1..=counted(row).len() {
            want.insert((ri, c));
        }
    }
    let mut seen = BTreeSet::new();
    let kerf = n.settings.kerf;
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    let mut placed = 0;
    for (pi, p) in n.pieces.iter().enumerate() {
        let bad = |what: String| Err(format!("piece {} ({}): {what}", p.index, p.stock));
        if p.index != pi + 1 || p.placements.is_empty() {
            return bad("numbered wrong or empty".into());
        }
        if !(p.kerf == kerf && p.trim == effective_trim(n.settings.trim, kerf)) {
            return bad("kerf or trim is not the settings'".into());
        }
        let [x0, y0, x1, y1] = p.usable;
        for (k, q) in p.placements.iter().enumerate() {
            let row = list
                .rows
                .get(q.row)
                .ok_or_else(|| format!("piece {}: no row {}", p.index, q.row))?;
            if !seen.insert((q.row, q.copy)) {
                return bad(format!("{} placed twice", q.tag));
            }
            if row.uncounted.is_some() {
                return bad(format!("{} is counted nowhere but placed", q.tag));
            }
            if counted(row).get(q.copy.wrapping_sub(1)) != Some(&q.item) {
                return bad(format!("{} is not its row's piece", q.tag));
            }
            let (w, l) = (row.size[1], row.size[2]);
            let dims_ok = if q.turned {
                q.dx == w && q.dy == l
            } else {
                q.dx == l && q.dy == w
            };
            if !dims_ok || !(q.x.is_finite() && q.y.is_finite()) {
                return bad(format!("{} is not its part's width and length", q.tag));
            }
            let along_grain = p.grain == StockGrain::Length && row.grain != Grain::None && w != l;
            if along_grain && q.turned != (row.grain == Grain::Width) {
                return bad(format!("{}'s grain is across the stock's", q.tag));
            }
            if q.x < x0 - EPS || q.y < y0 - EPS || q.x + q.dx > x1 + EPS || q.y + q.dy > y1 + EPS {
                return bad(format!("{} is outside the usable area", q.tag));
            }
            let t = row.size[0];
            let thick_ok = match p.kind {
                StockKind::Sheet => {
                    (p.thickness - t).abs() <= SHEET_THICKNESS_SLACK + STOCK_EPSILON
                }
                StockKind::Board => p.thickness >= t - STOCK_EPSILON,
            };
            if !thick_ok {
                return bad(format!("{} is not this stock's thickness", q.tag));
            }
            for r in &p.placements[k + 1..] {
                let apart = q.x + q.dx + kerf <= r.x + EPS
                    || r.x + r.dx + kerf <= q.x + EPS
                    || q.y + q.dy + kerf <= r.y + EPS
                    || r.y + r.dy + kerf <= q.y + EPS;
                if !apart {
                    return bad(format!(
                        "{} and {} overlap or are closer than the kerf",
                        q.tag, r.tag
                    ));
                }
            }
        }
        // No cut crosses a part.
        for c in &p.cuts {
            if c.n == 0 || !(c.at.is_finite() && c.from <= c.to + EPS) {
                return bad(format!("cut {} is malformed", c.n));
            }
            for q in &p.placements {
                let (span_lo, span_hi, lo, hi) = match c.dir {
                    CutDir::Rip => (q.x, q.x + q.dx, q.y, q.y + q.dy),
                    CutDir::Crosscut => (q.y, q.y + q.dy, q.x, q.x + q.dx),
                };
                let on_line = span_hi > c.from + EPS && span_lo < c.to - EPS;
                let crosses = hi > c.at + EPS && lo < c.at + kerf - EPS;
                if on_line && crosses {
                    return bad(format!("cut {} crosses {}", c.n, q.tag));
                }
            }
        }
        if p.cuts
            .iter()
            .enumerate()
            .any(|(k, c)| c.n as usize != k + 1)
        {
            return bad("cuts are not numbered in order".into());
        }
        let area: f64 = p.placements.iter().map(|q| q.dx * q.dy).sum();
        if !close(area, p.parts_area)
            || !close(p.stock_area, p.length * p.width)
            || !close(p.waste_area, p.stock_area - p.parts_area)
            || !close(p.efficiency, p.parts_area / p.stock_area)
            || p.efficiency > 1.0 + 1e-9
        {
            return bad("areas or efficiency do not add up".into());
        }
        if !p.guillotine && (n.algorithm == NestAlgorithm::Guillotine || p.kind == StockKind::Board)
        {
            return bad("a guillotine or board layout does not come apart in straight cuts".into());
        }
        placed += p.placements.len();
    }
    for u in &n.unplaced {
        let row = list
            .rows
            .get(u.row)
            .ok_or_else(|| format!("unplaced: no row {}", u.row))?;
        if !seen.insert((u.row, u.copy)) {
            return Err(format!(
                "{} is placed and reported, or reported twice",
                u.tag
            ));
        }
        if counted(row).get(u.copy.wrapping_sub(1)) != Some(&u.item) {
            return Err(format!("unplaced {} is not its row's piece", u.tag));
        }
        // Lane 15 fixes: a flat face or an open piece thicker than any board is reported
        // as exactly that, and nothing else is.
        let says = row.uncounted.map(|x| match x {
            crate::cutlist::Uncounted::Flat => UnplacedReason::Flat,
            crate::cutlist::Uncounted::NotABoard => UnplacedReason::NotABoard,
        });
        let reason_uncounted = matches!(u.reason, UnplacedReason::Flat | UnplacedReason::NotABoard);
        if says.is_some() != reason_uncounted || says.is_some_and(|r| r != u.reason) {
            return Err(format!(
                "unplaced {} reported {:?}, its row says {:?}",
                u.tag, u.reason, row.uncounted
            ));
        }
        if let (Some(name), UnplacedReason::TooLarge | UnplacedReason::Grain) = (&u.stock, u.reason)
        {
            let s = n
                .library
                .find(name)
                .ok_or_else(|| format!("unplaced {} names a missing stock", u.tag))?
                .1;
            let trim = effective_trim(n.settings.trim, kerf);
            let (uw, uh) = match s.kind {
                StockKind::Sheet => (s.length - 2.0 * trim, s.width - 2.0 * trim),
                StockKind::Board => (s.length - 2.0 * trim, s.width),
            };
            let holds = |dx: f64, dy: f64| dx <= uw + EPS && dy <= uh + EPS;
            let (w, l) = (row.size[1], row.size[2]);
            let free = s.grain == StockGrain::None || row.grain == Grain::None;
            let fits_allowed = if free {
                holds(l, w) || holds(w, l)
            } else if row.grain == Grain::Width {
                holds(w, l)
            } else {
                holds(l, w)
            };
            let fits_any = holds(l, w) || holds(w, l);
            let ok = match u.reason {
                UnplacedReason::TooLarge => !fits_any,
                _ => fits_any && !fits_allowed,
            };
            if !ok {
                return Err(format!("{} reported {:?} but it is not", u.tag, u.reason));
            }
        }
    }
    if seen != want {
        return Err(format!(
            "pieces in {} but placed or reported {}",
            want.len(),
            seen.len()
        ));
    }
    let t = &n.totals;
    let sheets = n
        .pieces
        .iter()
        .filter(|p| p.kind == StockKind::Sheet)
        .count();
    let boards = n.pieces.len() - sheets;
    if t.parts_in != want.len()
        || t.placed != placed
        || t.unplaced != n.unplaced.len()
        || t.parts_in != t.placed + t.unplaced
        || t.sheets != sheets
        || t.boards != boards
        || t.groups.iter().map(|g| g.count).sum::<usize>() != n.pieces.len()
        || t.groups.iter().map(|g| g.parts).sum::<usize>() != placed
        || !close(t.parts_area, n.pieces.iter().map(|p| p.parts_area).sum())
        || !close(t.stock_area, n.pieces.iter().map(|p| p.stock_area).sum())
        || t.efficiency > 1.0 + 1e-9
    {
        return Err("totals do not add up".into());
    }
    Ok(())
}

/// Materials and paint (lane 10) of any committed model: every face can be sampled and
/// shows what the view mesh draws; texture coordinates are finite, one pair per vertex,
/// and present exactly on faces whose material has an image the model holds.
pub fn check_looks(m: &Model, mesh: &crate::mesh::ViewMesh) -> Result<(), String> {
    if mesh.uvs.len() * 3 != mesh.positions.len() * 2 {
        return Err("view mesh texture coordinates are not two per vertex".into());
    }
    if mesh.uvs.iter().any(|u| !u.is_finite()) {
        return Err("a texture coordinate is not finite".into());
    }
    if mesh.edge_normals.len() != mesh.edges.len() {
        return Err("view mesh edge normals are not six per edge".into());
    }
    for (id, o) in m.objects() {
        for f in o.brep().persistent_face_ids() {
            let s = m
                .sample(id, f)
                .map_err(|e| format!("sampling face {f} of object {id}: {e}"))?;
            if s.as_deref() != m.face_material(id, f) {
                return Err("a sample differs from what the face shows".into());
            }
            if let Some(name) = &s
                && !m.materials().contains_key(name)
            {
                return Err(format!(
                    "face {f} of object {id} shows an undefined material"
                ));
            }
            let textured = m.face_texture(id, f).is_some();
            let uvs = crate::mesh::face_uvs(m, id, f, &Default::default());
            if textured != uvs.is_some() {
                return Err("a face's texture and its coordinates disagree".into());
            }
            if uvs.is_some_and(|u| u.iter().any(|x| !x.is_finite())) {
                return Err("a face's texture coordinates are not finite".into());
            }
        }
    }
    for p in &mesh.parts {
        if p.material
            .as_deref()
            .is_some_and(|n| !m.materials().contains_key(n))
        {
            return Err("the view mesh names an undefined material".into());
        }
    }
    Ok(())
}

/// The placement determinant of the innermost component instance around node `n` (or `n`
/// itself), if there is one.
fn instance_det(m: &Model, mut at: Option<u64>) -> Option<f64> {
    use crate::nodes::NodeKind;
    while let Some(n) = at {
        let node = m.node(n)?;
        if matches!(node.kind, NodeKind::Instance(_)) {
            return Some(node.xform.determinant());
        }
        at = node.member.parent();
    }
    None
}

/// The cut list (lane 7) of any committed model: it never panics, it is deterministic, and
/// it agrees with a brute-force reading of the world: every solid is in exactly one piece;
/// a component part's quantity is the number of its instances in the world (and the
/// roll-up through definitions); a piece is drawn exactly when the model draws it; handed
/// pieces are the mirrored instances; sizes are finite and in lumber order; Visible only
/// counts exactly the drawn pieces.
pub fn check_cut_list(m: &Model) -> Result<(), String> {
    use crate::cutlist::{CutOptions, PartKey, Rounding};
    use crate::nodes::{Item, NodeKind};
    use std::collections::{BTreeMap, BTreeSet};
    let run = |o: CutOptions| {
        catch_unwind(AssertUnwindSafe(|| m.cut_list(&o)))
            .map_err(|p| format!("cut list panicked: {}", panic_message(p)))
    };
    let all = run(CutOptions::default())?;
    if run(CutOptions::default())?.hash != all.hash {
        return Err("cut list: not deterministic".into());
    }
    let visible = run(CutOptions {
        visible_only: true,
        ..Default::default()
    })?;
    run(CutOptions {
        rounding: Rounding::HalfMm,
        visible_only: false,
    })?;
    let g = m.graph();
    let counts = m.part_counts();
    let mut seen = BTreeSet::new();
    let mut per_def: BTreeMap<u64, usize> = BTreeMap::new();
    for row in &all.rows {
        let bad = |what: &str| Err(format!("cut list row {:?}: {what}", row.name));
        if row.total != row.occurrences.len() || row.qty != row.total {
            return bad("quantity is not its pieces");
        }
        let shown = row.occurrences.iter().filter(|o| o.visible).count();
        if row.hidden != row.total - shown {
            return bad("hidden count is wrong");
        }
        let ordered = row.size.iter().all(|s| s.is_finite() && *s >= 0.0)
            && row.size[0] <= row.size[1]
            && row.size[1] <= row.size[2]
            && row.steps[0] <= row.steps[1]
            && row.steps[1] <= row.steps[2];
        if !ordered {
            return bad("sizes are not finite thickness <= width <= length");
        }
        if !(row.board_feet.is_finite() && row.board_feet >= 0.0 && row.area_m2 >= 0.0)
            || (row.sheet && row.board_feet != 0.0)
        {
            return bad("board feet or area is wrong");
        }
        let mut handed = 0;
        for o in &row.occurrences {
            let (drawn, det) = match o.item {
                Item::Object(id) => {
                    if !seen.insert(id) {
                        return bad("a solid is in two pieces");
                    }
                    let obj = m
                        .object(id)
                        .ok_or_else(|| "cut list names a missing solid".to_string())?;
                    let det = match row.key {
                        PartKey::Member { .. } => instance_det(m, obj.membership().parent()),
                        _ => None,
                    };
                    (m.is_visible(id), det)
                }
                Item::Node(n) => {
                    let node = m
                        .node(n)
                        .ok_or_else(|| "cut list names a missing node".to_string())?;
                    let solids: Vec<u64> = g
                        .objects
                        .iter()
                        .filter(|o| o.parent == Some(n))
                        .map(|o| o.id)
                        .collect();
                    // Lane 15: non-solid meshes with faces make a part too (flagged unless
                    // they heal closed).
                    let meshes: Vec<&crate::meshbuild::LooseMesh> = m
                        .meshes()
                        .into_iter()
                        .map(|(_, x)| x)
                        .filter(|x| x.member.parent() == Some(n) && !x.faces.is_empty())
                        .collect();
                    if (solids.is_empty() && meshes.is_empty())
                        || g.nodes.iter().any(|x| x.parent == Some(n))
                    {
                        return bad("a piece's group or instance is not a part container");
                    }
                    let open = meshes
                        .iter()
                        .any(|x| !crate::meshbuild::heal_check(&x.positions, &x.faces).closed);
                    if open && !row.not_solid {
                        return bad("a piece of open meshes is not flagged not solid");
                    }
                    for id in solids {
                        if !seen.insert(id) {
                            return bad("a solid is in two pieces");
                        }
                    }
                    let det = match (row.key, node.kind) {
                        (PartKey::Definition(d), NodeKind::Instance(e)) => {
                            if m.definition_id(e) != d {
                                return bad("an instance of another definition");
                            }
                            *per_def.entry(d).or_default() += 1;
                            Some(node.xform.determinant())
                        }
                        (PartKey::Member { .. }, NodeKind::Group) => {
                            instance_det(m, node.member.parent())
                        }
                        (PartKey::Node(_), NodeKind::Group) => None,
                        _ => return bad("a piece of the wrong kind"),
                    };
                    (m.node_visible(n), det)
                }
            };
            if drawn != o.visible {
                return bad("a piece's visibility is not the model's");
            }
            if det.is_some_and(|d| d < 0.0) {
                handed += 1;
            }
        }
        if handed != row.handed {
            return bad("handed count is not the mirrored instances");
        }
    }
    // Lane 15: faces outside parts are the meshes' faces whose container is the model or an
    // assembly (a group or instance holding groups or instances).
    let loose: usize = m
        .meshes()
        .into_iter()
        .map(|(_, x)| x)
        .filter(|x| match x.member.parent() {
            None => true,
            Some(p) => g.nodes.iter().any(|y| y.parent == Some(p)),
        })
        .map(|x| x.faces.len())
        .sum();
    if loose != all.totals.loose_faces {
        return Err(format!(
            "cut list: {} faces outside parts, the model has {loose}",
            all.totals.loose_faces
        ));
    }
    // Lane 15 fixes: the loose faces are split between the open context (what group_loose
    // acts on) and the places that hold the rest, each place once.
    let here: usize = m
        .meshes()
        .into_iter()
        .map(|(_, x)| x)
        .filter(|x| {
            x.member.parent() == m.context()
                && match x.member.parent() {
                    None => true,
                    Some(p) => g.nodes.iter().any(|y| y.parent == Some(p)),
                }
        })
        .map(|x| x.faces.len())
        .sum();
    let t = &all.totals;
    let placed: usize = t.loose_places.iter().map(|p| p.faces).sum();
    if here != t.loose_here || t.loose_here + placed != t.loose_faces {
        return Err(format!(
            "cut list: {} faces outside parts here (model {here}), {placed} elsewhere, {} in all",
            t.loose_here, t.loose_faces
        ));
    }
    let mut nodes_seen = BTreeSet::new();
    if t.loose_places
        .iter()
        .any(|p| p.faces == 0 || p.node == m.context() || !nodes_seen.insert(p.node))
    {
        return Err("cut list: a place of loose faces is empty, here, or listed twice".into());
    }
    // Rows counted nowhere carry nothing into the totals, and say why truthfully.
    for row in &all.rows {
        let Some(u) = row.uncounted else { continue };
        if row.board_feet != 0.0 || row.area_m2 != 0.0 || row.sheet {
            return Err(format!(
                "cut list: {} is uncounted but carries a total",
                row.name
            ));
        }
        let ok = match u {
            crate::cutlist::Uncounted::Flat => row.size[0] <= crate::cutlist::NO_THICKNESS,
            crate::cutlist::Uncounted::NotABoard => {
                row.not_solid && row.size[0] > crate::cutlist::THICKEST_LUMBER
            }
        };
        if !ok {
            return Err(format!("cut list: {} is {u:?} but it is not", row.name));
        }
    }
    if t.uncounted != all.rows.iter().filter(|r| r.uncounted.is_some()).count() {
        return Err("cut list: the uncounted total is not its rows".into());
    }
    let solids: BTreeSet<u64> = m.object_ids().into_iter().collect();
    if seen != solids {
        return Err(format!(
            "cut list: pieces cover {} solids, the model has {}",
            seen.len(),
            solids.len()
        ));
    }
    // Brute force: every instance of a part definition in the world is one piece, and the
    // roll-up through definitions says the same.
    for (d, n) in &per_def {
        let world = m
            .nodes()
            .iter()
            .filter(|(_, x)| matches!(x.kind, NodeKind::Instance(e) if m.definition_id(e) == *d))
            .count();
        if world != *n || counts.get(d) != Some(n) {
            return Err(format!(
                "cut list: definition {d} has {n} pieces, {world} instances in the world, roll-up {:?}",
                counts.get(d)
            ));
        }
    }
    let drawn: usize = all
        .rows
        .iter()
        .filter(|r| r.included)
        .map(|r| r.total - r.hidden)
        .sum();
    let counted: usize = all.rows.iter().filter(|r| r.included).map(|r| r.qty).sum();
    if all.totals.pieces != counted
        || all.totals.excluded != all.rows.iter().filter(|r| !r.included).count()
    {
        return Err("cut list: the totals count a dropped part".into());
    }
    if visible.totals.pieces != drawn || visible.rows.iter().any(|r| r.qty == 0 || r.hidden != 0) {
        return Err("cut list: Visible only does not count exactly the drawn pieces".into());
    }
    Ok(())
}

/// Run `ops` (any commands, undo and redo included) on an empty document and check
/// every invariant after each one. `Err` describes the first violation, naming the op.
///
/// Beyond [`check_model`]: a refused command changes nothing (state hash, op log, undo
/// depth); after an accepted edit, undo restores the previous state exactly and redo
/// returns to the new one; at the end, replaying the document's own log reproduces its
/// state and log hashes, and a second run from scratch lands on the same state.
pub fn check_sequence(ops: &[Op]) -> Result<RunSummary, String> {
    let mut d = Document::new();
    let mut summary = RunSummary::default();
    for (i, op) in ops.iter().enumerate() {
        let before = d.hash();
        let (log, depth) = (d.log().len(), d.depth());
        let attempt = catch_unwind(AssertUnwindSafe(|| {
            let mut next = d.clone();
            let r = next.apply(op);
            (next, r)
        }));
        match attempt {
            Err(p) => return Err(format!("op {i} {op:?} panicked: {}", panic_message(p))),
            Ok((next, Ok(_))) => {
                check_model(next.model()).map_err(|e| format!("op {i} {op:?}: {e}"))?;
                // Navigation (opening and closing groups) is not an undo step.
                if !matches!(op, Op::Undo | Op::Redo) && !op.is_navigation() {
                    let mut probe = next.clone();
                    probe
                        .undo()
                        .map_err(|e| format!("op {i} {op:?}: undo failed: {e}"))?;
                    if probe.hash() != before {
                        return Err(format!("op {i} {op:?}: undo did not restore the state"));
                    }
                    probe
                        .redo()
                        .map_err(|e| format!("op {i} {op:?}: redo failed: {e}"))?;
                    if probe.hash() != next.hash() {
                        return Err(format!("op {i} {op:?}: redo did not return to the state"));
                    }
                }
                d = next;
                summary.accepted += 1;
            }
            Ok((next, Err(e))) => {
                if e.is_bug() {
                    return Err(format!("op {i} {op:?}: validator rejected the result: {e}"));
                }
                if next.hash() != before
                    || d.hash() != before
                    || next.log().len() != log
                    || next.depth() != depth
                {
                    return Err(format!(
                        "op {i} {op:?}: refused ({e}) but the document changed"
                    ));
                }
                summary.refused += 1;
            }
        }
    }
    // The log is the history: replaying it lands on the same state and the same log.
    let replayed =
        Document::replay(d.log()).map_err(|(i, e)| format!("log replay refused entry {i}: {e}"))?;
    if replayed.hash() != d.hash() || replayed.log_hash() != d.log_hash() {
        return Err(format!(
            "log replay diverged: {} vs {}",
            replayed.hash_hex(),
            d.hash_hex()
        ));
    }
    // Determinism: a second run of the raw commands lands on the same document.
    let mut again = Document::new();
    for op in ops {
        let _ = again.apply(op);
    }
    if again.hash() != d.hash() {
        return Err(format!(
            "non-deterministic: {} vs {}",
            again.hash_hex(),
            d.hash_hex()
        ));
    }
    summary.objects = d.model().objects().len();
    Ok(summary)
}

/// A repro file: the failing sequence plus context.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Repro {
    pub name: String,
    pub error: String,
    /// Boolean plane snapping as the run that found it had it (`Some(false)`: a
    /// `FUZZ_PLANE_SNAP=0` run). Absent means on, the default; the replay test sets it
    /// for the file and puts it back afterwards, so a find made with snapping off fails
    /// the same way when it is replayed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane_snap: Option<bool>,
    pub ops: Vec<Op>,
}

impl Repro {
    /// Replay through [`check_sequence`] with the repro's plane snapping, restoring the
    /// thread's setting afterwards whatever happens.
    pub fn check(&self) -> Result<RunSummary, String> {
        let was = crate::snap::plane_snapping();
        crate::snap::set_plane_snapping(self.plane_snap.unwrap_or(true));
        let r = catch_unwind(AssertUnwindSafe(|| check_sequence(&self.ops)));
        crate::snap::set_plane_snapping(was);
        r.unwrap_or_else(|p| Err(format!("panicked: {}", panic_message(p))))
    }
}

/// Where the fuzzer writes a failing sequence before anyone has looked at it: a
/// subdirectory of the repro directory that git ignores (`tests/repro/pending/`). The
/// replay test reads only the repro directory itself, so a pending find never breaks
/// `replay_all_repros` for everyone else; promoting it (rename, fix, move up a level,
/// commit) is the step that makes it a regression case (`tests/repro/README.md`).
#[cfg(not(target_arch = "wasm32"))]
pub fn pending_dir(repro_dir: &std::path::Path) -> std::path::PathBuf {
    repro_dir.join("pending")
}

/// Write a failing sequence to `dir/<name>.json` (overwriting: when proptest shrinks, the
/// last write is the minimal case).
#[cfg(not(target_arch = "wasm32"))]
pub fn write_repro(
    dir: &std::path::Path,
    name: &str,
    error: &str,
    ops: &[Op],
) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{name}.json"));
    let repro = Repro {
        name: name.to_string(),
        error: error.to_string(),
        plane_snap: (!crate::snap::plane_snapping()).then_some(false),
        ops: ops.to_vec(),
    };
    let json = serde_json::to_string_pretty(&repro).map_err(std::io::Error::other)?;
    std::fs::write(&path, json + "\n")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_sequence_passes() {
        let ops = vec![
            Op::NewSketch {
                origin: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
            },
            Op::AddRectangle {
                sketch: 1,
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            Op::AddRectangle {
                sketch: 1,
                x: 0.5,
                y: 0.5,
                w: 1.0,
                h: 1.0,
            }, // crossing: splits into three regions
            Op::Extrude {
                sketch: 1,
                region: 1,
                distance: 0.5,
            },
            Op::PushPull {
                object: 1,
                face: 2, // the cap (persistent id 2)
                distance: 0.25,
            },
        ];
        let s = check_sequence(&ops).unwrap();
        assert_eq!((s.accepted, s.refused, s.objects), (5, 0, 1));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn repro_round_trip() {
        let dir = std::env::temp_dir().join(format!("sawhorse-repro-{}", std::process::id()));
        let ops = vec![Op::NewSketch {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        }];
        let path = write_repro(&dir, "pending-test", "example", &ops).unwrap();
        let back: Repro = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.ops, ops);
        assert_eq!(back.error, "example");
        std::fs::remove_dir_all(&dir).ok();
    }
}

// ---- boolean differential cases (tests/booleans/cases.json) ----------------------------
//
// The same case file drives the Rust test (`tests/booleans.rs`, hand-computed expectations)
// and the Node harness (`scripts/boolean-diff.mjs`), which runs each case through the WASM
// kernel and through npm `manifold-3d` (the C++ Manifold) on the very same operand meshes.

/// One operand: a box or a prism (outer ring, optional holes, between two heights), with
/// an optional loop imprinted on its top face (user-drawn edges to carry through).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SolidSpec {
    #[serde(rename = "box", default, skip_serializing_if = "Option::is_none")]
    pub cuboid: Option<[f64; 6]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prism: Option<PrismSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imprint_top: Option<Vec<[f64; 2]>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PrismSpec {
    pub outer: Vec<[f64; 2]>,
    #[serde(default)]
    pub holes: Vec<Vec<[f64; 2]>>,
    pub z0: f64,
    pub z1: f64,
}

/// Expected outcome. `refused`: the kernel refuses (typed); `volume`, `area`, `shells` and
/// `genus` then describe what a mesh engine reports.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Expect {
    #[serde(default)]
    pub empty: bool,
    #[serde(default)]
    pub refused: bool,
    pub volume: Option<f64>,
    pub area: Option<f64>,
    pub shells: Option<usize>,
    pub genus: Option<i64>,
    pub faces: Option<usize>,
    pub holes: Option<usize>,
    pub user_edges: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BoolCase {
    pub name: String,
    pub op: crate::boolean::BoolOp,
    pub a: SolidSpec,
    pub b: SolidSpec,
    pub expect: Expect,
}

/// What the kernel produced for a case.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct CaseResult {
    pub empty: bool,
    pub refused: Option<String>,
    pub volume: f64,
    pub area: f64,
    pub shells: usize,
    pub genus: i64,
    pub faces: usize,
    pub holes: usize,
    pub user_edges: usize,
}

/// Build an operand through the ordinary ops (sketch, polyline, extrude, imprint).
pub fn build_solid(s: &SolidSpec) -> Result<crate::object::Object, String> {
    let (outer, holes, z0, z1) = match (&s.cuboid, &s.prism) {
        (Some(b), None) => (
            vec![[b[0], b[1]], [b[3], b[1]], [b[3], b[4]], [b[0], b[4]]],
            Vec::new(),
            b[2],
            b[5],
        ),
        (None, Some(p)) => (p.outer.clone(), p.holes.clone(), p.z0, p.z1),
        _ => return Err("an operand is exactly one of box / prism".into()),
    };
    let mut m = Model::new();
    let e = |e: crate::error::KernelError| e.to_string();
    m.apply(&Op::NewSketch {
        origin: [0.0, 0.0, z0],
        normal: [0.0, 0.0, 1.0],
    })
    .map_err(e)?;
    for ring in std::iter::once(&outer).chain(holes.iter()) {
        m.apply(&Op::AddPolyline {
            sketch: 1,
            points: ring.clone(),
            closed: true,
        })
        .map_err(e)?;
    }
    let s1 = m.sketch(1).ok_or("sketch")?;
    let region = s1
        .regions()
        .max_by(|a, b| s1.region_area(a.1).total_cmp(&s1.region_area(b.1)))
        .map(|(id, _)| crate::ids::to_public(id))
        .ok_or("no region")?;
    m.apply(&Op::Extrude {
        sketch: 1,
        region,
        distance: z1 - z0,
    })
    .map_err(e)?;
    if let Some(pts) = &s.imprint_top {
        m.apply(&Op::Imprint {
            object: 1,
            face: 2, // the top (persistent id 2)
            points: pts.iter().map(|p| [p[0], p[1], z1]).collect(),
        })
        .map_err(e)?;
    }
    m.object(1).cloned().ok_or_else(|| "no object".into())
}

/// Run a case through `Object::combine` (the push/pull boolean pipeline).
pub fn run_boolean_case(c: &BoolCase) -> Result<CaseResult, String> {
    let a = build_solid(&c.a)?;
    let b = build_solid(&c.b)?;
    match a.combine(&b, c.op) {
        Ok(None) => Ok(CaseResult {
            empty: true,
            ..Default::default()
        }),
        Ok(Some(o)) => {
            let r = o.validate().map_err(|e| e.to_string())?;
            Ok(CaseResult {
                empty: false,
                refused: None,
                volume: o.volume(),
                area: o.surface_area(),
                shells: r.shells.len(),
                genus: r.shells.iter().map(|s| s.genus.unwrap_or(0)).sum(),
                faces: r.faces,
                holes: r.holes,
                user_edges: o.brep().edges().filter(|(_, e)| e.user).count(),
            })
        }
        Err(e) => Ok(CaseResult {
            refused: Some(e.to_string()),
            ..Default::default()
        }),
    }
}

/// Check a result against a case's expectations; `Err` lists every mismatch.
pub fn check_case(c: &BoolCase, r: &CaseResult) -> Result<(), String> {
    let x = &c.expect;
    let mut bad = Vec::new();
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs().max(1.0);
    if x.refused != r.refused.is_some() {
        bad.push(format!(
            "refused: expected {}, got {:?}",
            x.refused, r.refused
        ));
    }
    if x.refused {
        return if bad.is_empty() {
            Ok(())
        } else {
            Err(bad.join("; "))
        };
    }
    if x.empty != r.empty {
        bad.push(format!("empty: expected {}, got {}", x.empty, r.empty));
    }
    if let Some(v) = x.volume
        && !near(r.volume, v)
    {
        bad.push(format!("volume {} (expected {v})", r.volume));
    }
    if let Some(v) = x.area
        && !near(r.area, v)
    {
        bad.push(format!("area {} (expected {v})", r.area));
    }
    for (what, got, want) in [
        ("shells", r.shells as i64, x.shells.map(|v| v as i64)),
        ("genus", r.genus, x.genus),
        ("faces", r.faces as i64, x.faces.map(|v| v as i64)),
        ("holes", r.holes as i64, x.holes.map(|v| v as i64)),
        (
            "user edges",
            r.user_edges as i64,
            x.user_edges.map(|v| v as i64),
        ),
    ] {
        if let Some(w) = want
            && got != w
        {
            bad.push(format!("{what} {got} (expected {w})"));
        }
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad.join("; "))
    }
}

/// A small PNG (8-bit RGB, uncompressed deflate) of `w` x `h` pixels: a checker of `rgb`
/// and a darker shade of it, so a texture's orientation shows. For tests and fixtures.
pub fn tiny_png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let dark = rgb.map(|c| c / 2);
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0u8);
        for x in 0..w {
            raw.extend(if (x + y) % 2 == 0 { rgb } else { dark });
        }
    }
    png_from_raw(w, h, &raw)
}

/// PNG bytes from filtered scanlines (`raw`: each row a filter byte then RGB triples).
pub fn png_from_raw(w: u32, h: u32, raw: &[u8]) -> Vec<u8> {
    fn crc32(d: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in d {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend(crc32(&c).to_be_bytes());
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let n = b.len() as u16;
        z.extend(n.to_le_bytes());
        z.extend((!n).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut s2) = (1u32, 0u32);
    for &b in raw {
        a = (a + u32::from(b)) % 65521;
        s2 = (s2 + a) % 65521;
    }
    z.extend(((s2 << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}
