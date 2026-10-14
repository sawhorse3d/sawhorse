//! How long a section cut takes (lane 19c): the measurement behind "the cut is not slower
//! than twice the cut it replaced". Ignored by default; run it in release:
//!
//! ```text
//! scripts/cargo.sh test --release -p sawhorse-kernel --test section_perf -- --ignored --nocapture
//! ```
//!
//! It times [`Model::section_cut`] (what the view, the drawings and the slice ask for) and,
//! for the record, [`Model::kept_part`] (the fuzzer's closure oracle) on:
//!
//! - the lane-9 bookshelf of the section golden (`tests/golden/section.json`), its two planes;
//! - when `SAWHORSE_PERF_OP` names a JSON file holding one kernel op (an import: the op the
//!   app sends for a `.skp` file, written by the `.skp` import's own code), that model, cut by
//!   four planes through its bounding box (a vertical plane across x, one across y, and plan
//!   cuts at a third and a tenth of its height). The file is never part of the repository: the
//!   models measured are private SketchUp files (docs/DEVELOPMENT.md, "Private fixtures").
//!
//! Each line gives the median of the runs in milliseconds, the caps and their area, so two
//! kernels can be compared on the same machine and checked to cut the same thing.

use std::time::Instant;

use glam::DVec3;
use sawhorse_kernel::Document;
use sawhorse_kernel::model::{Model, Op, OpOutcome};

fn median_ms(mut runs: Vec<f64>) -> f64 {
    runs.sort_by(f64::total_cmp);
    runs[runs.len() / 2]
}

fn time_cuts(what: &str, m: &Model, sections: &[u64], n: usize) {
    for &s in sections {
        let mut cut_ms = Vec::with_capacity(n);
        let mut cut = None;
        for _ in 0..n {
            let t = Instant::now();
            let c = m.section_cut(s).expect("the cut");
            cut_ms.push(t.elapsed().as_secs_f64() * 1e3);
            cut = Some(c);
        }
        let mut kept_ms = Vec::with_capacity(n);
        for _ in 0..n.min(20) {
            let t = Instant::now();
            let _ = m.kept_part(s).expect("the kept part");
            kept_ms.push(t.elapsed().as_secs_f64() * 1e3);
        }
        let cut = cut.expect("one run");
        let rings: usize = cut.caps.iter().map(|c| 1 + c.holes.len()).sum();
        let points: usize = cut
            .caps
            .iter()
            .map(|c| c.outer.len() + c.holes.iter().map(Vec::len).sum::<usize>())
            .sum();
        println!(
            "[section-perf] {what}, section {s}: cut {:.3} ms (median of {n}), kept part {:.3} ms; {} caps, {rings} rings, {points} points, area {:.9} m2, hash {}",
            median_ms(cut_ms),
            median_ms(kept_ms),
            cut.caps.len(),
            cut.area,
            cut.hash
        );
    }
}

#[test]
#[ignore = "a timing, run on demand in release"]
fn section_cut_timings() {
    #[derive(serde::Deserialize)]
    struct Golden {
        ops: Vec<Op>,
    }
    let g: Golden = serde_json::from_str(include_str!("golden/section.json")).unwrap();
    let d = Document::replay(&g.ops).unwrap_or_else(|(i, e)| panic!("op {i}: {e}"));
    let m = d.model();
    let ids: Vec<u64> = m.sections().into_iter().map(|(id, _)| id).collect();
    println!(
        "[section-perf] bookshelf: {} solids, {} faces",
        m.objects().len(),
        m.stats().faces
    );
    time_cuts("bookshelf", m, &ids, 200);

    let Ok(path) = std::env::var("SAWHORSE_PERF_OP") else {
        println!("[section-perf] SAWHORSE_PERF_OP is not set: no large model measured");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let op: Op = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut d = Document::new();
    let t = Instant::now();
    d.apply(&op).unwrap_or_else(|e| panic!("{path}: {e}"));
    let load = t.elapsed().as_secs_f64();
    let m = d.model();
    let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    for (_, o) in m.objects() {
        for (_, v) in o.brep().vertices() {
            lo = lo.min(v.pos);
            hi = hi.max(v.pos);
        }
    }
    println!(
        "[section-perf] model: {} solids, {} faces, loaded in {load:.1} s, bounds {lo:?} to {hi:?}",
        m.objects().len(),
        m.stats().faces
    );
    let at = |f: [f64; 3]| (lo + (hi - lo) * DVec3::from_array(f)).to_array();
    let mut ids = Vec::new();
    for (origin, normal) in [
        (at([0.5, 0.5, 0.5]), [1.0, 0.0, 0.0]),
        (at([0.5, 0.37, 0.5]), [0.0, -1.0, 0.0]),
        (at([0.5, 0.5, 0.31]), [0.0, 0.0, -1.0]),
        (at([0.5, 0.5, 0.1]), [0.0, 0.0, -1.0]),
    ] {
        match d.apply(&Op::AddSection {
            origin,
            normal,
            name: None,
            active: false,
            hidden: false,
        }) {
            Ok(OpOutcome::SectionAdded { section }) => ids.push(section),
            o => panic!("{o:?}"),
        }
    }
    time_cuts("model", d.model(), &ids, 10);
}
