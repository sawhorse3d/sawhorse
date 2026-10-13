//! Lane 16: what the readers refuse, and how fast. Every case here used to allocate (or
//! loop) in proportion to what the file claimed, not to its size.

use std::time::Instant;

use sawhorse_import::{ImportOptions, read};

/// A raw Deflate stream inflating to `n` zero bytes, built in milliseconds (one fully
/// flushed run of blocks repeated, then a final empty block).
fn deflate_zeros(n: u64) -> Vec<u8> {
    use flate2::{Compress, Compression, FlushCompress};
    let chunk = n.min(64 << 20) as usize;
    let mut c = Compress::new(Compression::best(), false);
    let zeros = vec![0u8; chunk];
    let mut block = Vec::with_capacity(chunk / 500 + 1024);
    let mut fed = 0;
    while fed < chunk {
        let before = c.total_in() as usize;
        c.compress_vec(&zeros[fed..], &mut block, FlushCompress::Full)
            .unwrap();
        fed += c.total_in() as usize - before;
        if block.len() == block.capacity() {
            block.reserve(block.capacity());
        }
    }
    loop {
        let cap = block.capacity();
        c.compress_vec(&[], &mut block, FlushCompress::Full)
            .unwrap();
        if block.len() < cap {
            break;
        }
        block.reserve(cap);
    }
    let mut out = Vec::new();
    for _ in 0..(n / chunk as u64) {
        out.extend_from_slice(&block);
    }
    out.extend_from_slice(&[0x03, 0x00]);
    out
}

/// A zip of raw entries (name, method, data, declared size) with a well-formed directory.
fn raw_zip(entries: &[(&str, u16, Vec<u8>, u64)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cd = Vec::new();
    for (name, method, data, size) in entries {
        let off = out.len() as u32;
        for (sig, buf) in [(0x04034b50u32, &mut out), (0x02014b50u32, &mut cd)] {
            buf.extend_from_slice(&sig.to_le_bytes());
            if sig == 0x02014b50 {
                buf.extend_from_slice(&20u16.to_le_bytes());
            }
            buf.extend_from_slice(&20u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&method.to_le_bytes());
            buf.extend_from_slice(&[0; 4]);
            // A stored entry carries its real checksum; a deflated one here is refused before
            // its checksum could matter.
            let crc = if *method == 0 {
                crc32fast::hash(data)
            } else {
                0
            };
            buf.extend_from_slice(&crc.to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(*size as u32).to_le_bytes());
            buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            if sig == 0x02014b50 {
                buf.extend_from_slice(&[0; 10]);
                buf.extend_from_slice(&off.to_le_bytes());
            }
            buf.extend_from_slice(name.as_bytes());
            if sig == 0x04034b50 {
                buf.extend_from_slice(data);
            }
        }
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&cd);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn opts(name: &str) -> ImportOptions {
    ImportOptions {
        units: None,
        name: name.into(),
    }
}

#[test]
fn a_3mf_zip_bomb_is_refused_without_inflating() {
    let bomb = deflate_zeros(2 << 30);
    assert!(bomb.len() < 4 << 20);
    let file = raw_zip(&[("3D/3dmodel.model", 8, bomb, 2 << 30)]);
    let t = Instant::now();
    let e = read(None, &file, &opts("bomb.3mf")).unwrap_err();
    assert!(t.elapsed().as_millis() < 100, "{:?}", t.elapsed());
    assert!(e.to_string().contains("more than the"), "{e}");
    // The same stream declared small: refused one byte past the claim.
    let file = raw_zip(&[("3D/3dmodel.model", 8, deflate_zeros(64 << 20), 1000)]);
    let t = Instant::now();
    let e = read(None, &file, &opts("lie.3mf")).unwrap_err();
    assert!(t.elapsed().as_millis() < 100, "{:?}", t.elapsed());
    assert!(
        e.to_string().contains("not the 1000 its header says"),
        "{e}"
    );
}

#[test]
fn a_3mf_whose_components_fan_out_is_counted_before_it_is_built() {
    // 12 objects, each holding the next ten times, the last a triangle: 10^11 triangles.
    let mut objects = String::new();
    objects.push_str(r#"<object id="12" type="model"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>"#);
    for i in (1..12).rev() {
        objects.push_str(&format!(r#"<object id="{i}" type="model"><components>"#));
        for _ in 0..10 {
            objects.push_str(&format!(r#"<component objectid="{}"/>"#, i + 1));
        }
        objects.push_str("</components></object>");
    }
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>{objects}</resources><build><item objectid="1"/></build></model>"#
    );
    let file = raw_zip(&[(
        "3D/3dmodel.model",
        0,
        xml.clone().into_bytes(),
        xml.len() as u64,
    )]);
    let t = Instant::now();
    let e = read(None, &file, &opts("fan.3mf")).unwrap_err();
    assert!(t.elapsed().as_millis() < 1000, "{:?}", t.elapsed());
    assert!(e.to_string().contains("faces"), "{e}");
}

fn gltf(accessor: serde_json::Value, extra: serde_json::Value) -> Vec<u8> {
    let mut doc = serde_json::json!({
        "asset": {"version": "2.0"},
        "accessors": [accessor],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        "nodes": [{"mesh": 0}],
        "scenes": [{"nodes": [0]}]
    });
    if let serde_json::Value::Object(m) = extra {
        for (k, v) in m {
            doc[k] = v;
        }
    }
    doc.to_string().into_bytes()
}

#[test]
fn a_gltf_accessor_count_is_checked_before_anything_is_allocated() {
    // count = 2^31 with no buffer view: 6 billion zeros once; refused at once.
    let t = Instant::now();
    let e = read(
        None,
        &gltf(
            serde_json::json!({"type": "VEC3", "componentType": 5126, "count": 2147483648u64}),
            serde_json::json!({}),
        ),
        &opts("big.gltf"),
    )
    .unwrap_err();
    assert!(t.elapsed().as_millis() < 100, "{:?}", t.elapsed());
    assert!(
        e.to_string().contains("without data") || e.to_string().contains("nothing"),
        "{e}"
    );
    // With a 36-byte buffer: the claim runs past it.
    let b64 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let doc = gltf(
        serde_json::json!({"bufferView": 0, "type": "VEC3", "componentType": 5126, "count": 2147483648u64}),
        serde_json::json!({"buffers": [{"byteLength": 36, "uri": format!("data:application/octet-stream;base64,{b64}")}], "bufferViews": [{"buffer": 0, "byteLength": 36}]}),
    );
    let t = Instant::now();
    let e = read(None, &doc, &opts("lie.gltf")).unwrap_err();
    assert!(t.elapsed().as_millis() < 100, "{:?}", t.elapsed());
    assert!(
        e.to_string().contains("runs past its buffer") || e.to_string().contains("nothing"),
        "{e}"
    );
}

#[test]
fn a_gltf_node_reached_twice_is_refused_and_a_short_label_list_is_dropped() {
    // 40 nodes, each the child of the one before twice: 2^40 visits once.
    let nodes: Vec<serde_json::Value> = (0..40)
        .map(|i| {
            if i < 39 {
                serde_json::json!({"children": [i + 1, i + 1]})
            } else {
                serde_json::json!({})
            }
        })
        .collect();
    let doc = serde_json::json!({"asset": {"version": "2.0"}, "nodes": nodes, "scenes": [{"nodes": [0]}]});
    let t = Instant::now();
    let e = read(None, doc.to_string().as_bytes(), &opts("dag.gltf")).unwrap_err();
    assert!(t.elapsed().as_millis() < 100, "{:?}", t.elapsed());
    assert!(e.to_string().contains("more than once"), "{e}");
    // A _FACE_ID accessor with no labels: not an index past its end (it used to panic).
    let tri = "AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA";
    let doc = serde_json::json!({
        "asset": {"version": "2.0"},
        "buffers": [{"byteLength": 36, "uri": format!("data:application/octet-stream;base64,{tri}")}],
        "bufferViews": [{"buffer": 0, "byteLength": 36}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}, {"componentType": 5126, "count": 0, "type": "SCALAR"}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "_FACE_ID": 1}}]}],
        "nodes": [{"mesh": 0}],
        "scenes": [{"nodes": [0]}]
    });
    let p = read(None, doc.to_string().as_bytes(), &opts("labels.gltf")).unwrap();
    assert!(p.items[0].face_labels.is_empty());
}

#[test]
fn nan_and_infinite_coordinates_never_become_geometry() {
    // A binary STL whose only triangle has a NaN and an infinity: skipped with a note.
    let mut stl = vec![0u8; 84 + 50];
    stl[80..84].copy_from_slice(&1u32.to_le_bytes());
    stl[84 + 12..84 + 16].copy_from_slice(&f32::NAN.to_le_bytes());
    stl[84 + 16..84 + 20].copy_from_slice(&f32::INFINITY.to_le_bytes());
    let p = read(None, &stl, &opts("nan.stl")).unwrap_err();
    assert!(p.to_string().contains("nothing"), "{p}");
    // OBJ refuses a non-finite vertex outright.
    assert!(
        read(
            None,
            b"v nan 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
            &opts("nan.obj")
        )
        .is_err()
    );
    assert!(
        read(
            None,
            b"v inf 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
            &opts("inf.obj")
        )
        .is_err()
    );
}

#[test]
fn a_binary_stl_over_the_import_limit_is_refused_before_its_soup_is_built() {
    let n = sawhorse_kernel::limits::MAX_IMPORT_FACES + 1;
    let mut stl = vec![0u8; 84 + 50 * n];
    stl[80..84].copy_from_slice(&(n as u32).to_le_bytes());
    let t = Instant::now();
    let e = read(None, &stl, &opts("big.stl")).unwrap_err();
    assert!(t.elapsed().as_millis() < 100, "{:?}", t.elapsed());
    assert!(e.to_string().contains("faces"), "{e}");
}

#[test]
fn dxf_and_svg_strokes_are_held_to_the_import_limits() {
    let mut dxf = String::from("0\nSECTION\n2\nENTITIES\n");
    for i in 0..(sawhorse_kernel::limits::MAX_IMPORT_STROKES + 1) {
        dxf.push_str(&format!(
            "0\nLINE\n8\n0\n10\n{i}\n20\n0\n30\n0\n11\n{i}\n21\n1\n31\n0\n"
        ));
    }
    dxf.push_str("0\nENDSEC\n0\nEOF\n");
    let e = read(None, dxf.as_bytes(), &opts("lines.dxf")).unwrap_err();
    assert!(e.to_string().contains("lines and curves"), "{e}");
    let mut many = String::from("0\nSECTION\n2\nENTITIES\n");
    for _ in 0..(sawhorse_import::dxf::MAX_ENTITIES + 1) {
        many.push_str("0\nPOINT\n");
    }
    let e = read(None, many.as_bytes(), &opts("points.dxf")).unwrap_err();
    assert!(e.to_string().contains("entities"), "{e}");
    // An SVG path standing for more points than one import takes.
    let mut d = String::from("M0 0");
    for i in 0..(sawhorse_kernel::limits::MAX_IMPORT_STROKE_POINTS / 2) {
        d.push_str(&format!(" C{i} 1 {i} 2 {i} 3"));
    }
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><path d="{d}"/></svg>"#
    );
    let e = read(None, svg.as_bytes(), &opts("curves.svg")).unwrap_err();
    assert!(e.to_string().contains("points"), "{e}");
}

#[test]
fn the_most_strokes_one_import_takes_come_in_without_trying_every_pair() {
    // C3: 5,000 lines (the import limit) in one sketch. Each repair and edge check looks
    // only near where it looks (a sweep and grids), not at every pair of 5,000 edges.
    let n = sawhorse_kernel::limits::MAX_IMPORT_STROKES;
    let mut dxf = String::from("0\nSECTION\n2\nENTITIES\n");
    for i in 0..n {
        let (x, y) = ((i % 100) as f64 * 3.0, (i / 100) as f64 * 3.0);
        dxf.push_str(&format!(
            "0\nLINE\n8\n0\n10\n{x}\n20\n{y}\n30\n0\n11\n{}\n21\n{}\n31\n0\n",
            x + 1.0,
            y + 1.0
        ));
    }
    dxf.push_str("0\nENDSEC\n0\nEOF\n");
    let parsed = read(None, dxf.as_bytes(), &opts("lines.dxf")).unwrap();
    let (op, _) = parsed.into_op();
    let mut d = sawhorse_kernel::document::Document::with_ground_sketch();
    let t = Instant::now();
    d.apply_owned(op).unwrap();
    let took = t.elapsed();
    eprintln!("5,000 strokes: {took:?}");
    // About 0.5 s in a debug build (35 s before, one sketch copy, repair and region pass
    // per line); the bound leaves room for a loaded machine.
    assert!(took.as_secs() < 10, "{took:?}");
    // And the next edit of that sketch costs one pass over it, not a pass per pair.
    let t = Instant::now();
    d.apply(&sawhorse_kernel::Op::InsertSegment {
        sketch: d.model().sketch_ids()[0],
        a: [-1.0, -1.0],
        b: [-1.0, 290.0],
    })
    .unwrap();
    assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
}
