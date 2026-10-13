//! Section planes in picking and inference (lane 12): what an active plane cuts away is
//! neither picked nor snapped to, a click on the cap picks the solid, and the cut's corners
//! and edges snap like any other; turning section cuts off in the style brings it all back.

use glam::DVec3;
use sawhorse_inference::InferenceKind as K;
use sawhorse_inference::geom::PixelModel;
use sawhorse_inference::infer::{InferQuery, infer};
use sawhorse_inference::pick::{Entity, PickQuery, pick};
use sawhorse_inference::scene::{Scene, SurfaceRef};
use sawhorse_kernel::model::{Model, Op};

/// A 1 m cube from the origin, cut at x = 0.5 keeping x < 0.5.
fn cut_cube() -> Model {
    let mut m = Model::with_ground_sketch();
    m.apply(&Op::AddRectangle {
        sketch: 1,
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    })
    .unwrap();
    m.apply(&Op::Extrude {
        sketch: 1,
        region: 1,
        distance: 1.0,
    })
    .unwrap();
    m.apply(&Op::AddSection {
        origin: [0.5, 0.0, 0.0],
        normal: [-1.0, 0.0, 0.0],
        name: None,
        active: true,
        hidden: false,
    })
    .unwrap();
    m
}

/// Looking along -X at (y, z) from x = 10, parallel, `mpp` metres per pixel.
fn from_right(y: f64, z: f64, mpp: f64) -> (PixelModel, [f64; 3], [f64; 3]) {
    (
        PixelModel {
            ortho: true,
            scale: mpp,
            eye: [10.0, y, z],
            forward: [-1.0, 0.0, 0.0],
        },
        [10.0, y, z],
        [-1.0, 0.0, 0.0],
    )
}

fn pick_at(m: &Model, y: f64, z: f64) -> sawhorse_inference::pick::Picked {
    let (px, origin, dir) = from_right(y, z, 0.001);
    pick(
        &Scene::build(m),
        &PickQuery {
            origin,
            dir,
            px,
            point_px: 8.0,
            edge_px: 5.0,
            soft: false,
        },
    )
    .expect("a pick")
}

#[test]
fn the_cap_is_picked_and_what_is_cut_away_is_not() {
    let m = cut_cube();
    let p = pick_at(&m, 0.4, 0.6);
    assert_eq!(p.entity, Entity::Object { object: 1 });
    assert!(
        (p.point[0] - 0.5).abs() < 1e-12,
        "the cap at x = 0.5: {:?}",
        p.point
    );
    let s = p.surface.unwrap();
    assert!(matches!(
        s.at,
        SurfaceRef::Cap {
            object: 1,
            section: 1
        }
    ));
    // The cap faces the side that is cut away.
    assert!(s.frame.normal[0] > 0.99);
    // Sections off in the style: the cube's own right face again.
    let mut off = m.clone();
    off.apply(&Op::SetStyle {
        style: Some(sawhorse_kernel::looks::Style {
            section_cuts: false,
            ..Default::default()
        }),
    })
    .unwrap();
    let p = pick_at(&off, 0.4, 0.6);
    assert!(matches!(p.entity, Entity::Face { object: 1, .. }));
    assert!((p.point[0] - 1.0).abs() < 1e-12);
    // An inactive plane cuts nothing.
    let mut idle = m.clone();
    idle.apply(&Op::SetSectionActive {
        section: 1,
        active: false,
    })
    .unwrap();
    assert!((pick_at(&idle, 0.4, 0.6).point[0] - 1.0).abs() < 1e-12);
}

#[test]
fn the_cut_corners_snap_and_the_removed_edges_do_not() {
    let m = cut_cube();
    let s = Scene::build(&m);
    // Edges: the cube's 12, clipped (the four along x shortened, the four at x = 1 gone),
    // plus the cap's 4.
    assert_eq!(s.segs.len(), 12);
    assert!(
        s.segs
            .iter()
            .all(|g| g.a.x <= 0.5 + 1e-12 && g.b.x <= 0.5 + 1e-12)
    );
    // Hovering near the cap's top corner snaps to it.
    let (px, origin, dir) = from_right(0.003, 0.997, 0.001);
    let q = InferQuery {
        origin,
        dir,
        px,
        start: None,
        plane: None,
        lock: None,
        refs: vec![],
        anchors: vec![],
        grid: false,
        grid_step: None,
        point_px: 10.0,
        line_px: 7.0,
        soft: false,
    };
    let r = infer(&s, &q).unwrap();
    assert_eq!(r.kind, K::Endpoint);
    assert!((DVec3::from_array(r.point) - DVec3::new(0.5, 0.0, 1.0)).length() < 1e-12);
}
