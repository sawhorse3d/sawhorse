//! Deterministic replay with golden hashes.
//!
//! Each `tests/golden/*.json` holds a document op log (undo and redo included) and the
//! state hash replaying it must produce.
//! The same files are replayed through the WASM build by `scripts/golden-wasm.mjs`, so a
//! passing pair proves native and browser kernels agree bit for bit (after quantisation).
//!
//! After an intended kernel change: `UPDATE_GOLDEN=1 cargo test --test golden` (or
//! `just golden-update`), then review the diff like code.

use std::path::PathBuf;

use sawhorse_kernel::Document;
use sawhorse_kernel::model::Op;

#[derive(serde::Serialize, serde::Deserialize)]
struct Golden {
    name: String,
    description: String,
    ops: Vec<Op>,
    hash: String,
    stats: GoldenStats,
    /// The cut list's hash (1/16 in, everything counted): the WASM build must list the same
    /// parts (`scripts/golden-wasm.mjs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cut_list: Option<String>,
    /// The nesting's hash (the document's own stock and settings, everything counted): the
    /// WASM build must lay out the same stock (lane 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nesting: Option<String>,
    /// Each section plane's cut hash, in id order (lane 12): the WASM build must cut the same
    /// caps and outlines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sections: Option<Vec<String>>,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct GoldenStats {
    objects: usize,
    faces: usize,
    edges: usize,
    vertices: usize,
    genus: i64,
    watertight: bool,
}

#[test]
fn golden_hashes() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden");
    let update = std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no golden files in {}", dir.display());
    let mut mismatches = Vec::new();
    for f in files {
        let mut g: Golden = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        let d = Document::replay(&g.ops)
            .unwrap_or_else(|(i, e)| panic!("{}: op {i} failed: {e}", f.display()));
        // Replay twice: the hash must not depend on anything but the ops.
        assert_eq!(d.hash(), Document::replay(&g.ops).unwrap().hash());
        let m = d.model();
        let st = m.stats();
        let stats = GoldenStats {
            objects: st.objects,
            faces: st.faces,
            edges: st.edges,
            vertices: st.vertices,
            genus: st.genus,
            watertight: st.watertight,
        };
        let hash = m.hash_hex();
        let cut = m
            .cut_list(&sawhorse_kernel::cutlist::CutOptions::default())
            .hash;
        let nest = m
            .nest(&sawhorse_kernel::nesting::NestOptions::default())
            .hash;
        let cuts: Vec<String> = m
            .sections()
            .into_iter()
            .map(|(id, _)| m.section_cut(id).unwrap().hash)
            .collect();
        let cuts = (!cuts.is_empty()).then_some(cuts);
        if update {
            g.hash = hash;
            g.stats = stats;
            g.cut_list = Some(cut);
            g.nesting = Some(nest);
            g.sections = cuts;
            std::fs::write(&f, serde_json::to_string_pretty(&g).unwrap() + "\n").unwrap();
        } else if g.hash != hash
            || g.stats != stats
            || g.cut_list.as_ref() != Some(&cut)
            || g.nesting.as_ref() != Some(&nest)
            || g.sections != cuts
        {
            mismatches.push(format!(
                "{}: hash {} (golden {}), stats {:?} (golden {:?}), cut list {} (golden {:?}), nesting {} (golden {:?}), sections {:?} (golden {:?})",
                f.display(),
                hash,
                g.hash,
                stats,
                g.stats,
                cut,
                g.cut_list,
                nest,
                g.nesting,
                cuts,
                g.sections
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "golden mismatch (UPDATE_GOLDEN=1 to accept):\n{}",
        mismatches.join("\n")
    );
}
