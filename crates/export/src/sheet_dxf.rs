//! A drawing sheet as DXF (lane 12): the sheet's own primitives ([`crate::sheet`]) at paper
//! size, in the sheet's units (inches or millimetres, `$INSUNITS` 1 or 4), y up from the
//! paper's bottom-left corner. Plain entities only, so any CAD or CAM program reads it:
//! `LINE`s (hidden lines on the `DASHED` linetype), closed `LWPOLYLINE`s for rectangles,
//! image frames and the outlines of section fills, and `TEXT` (dimension values are text
//! here, never `DIMENSION` entities: what the sheet shows is what the file holds). One
//! layer per kind of thing: `VIEWS`, `HIDDEN`, `SECTION`, `DIMENSIONS`, `LABELS`,
//! `TABLES`, `IMAGES`, `LINES` and `SHEET` (the margin frame and the title block).

use sawhorse_kernel::sheets::PaperUnits;

use crate::dxf::{Dxf, num};
use crate::sheet::{Anchor, Prim, SheetRender, fit_text};

const IN: f64 = 0.0254;

const LAYERS: [&str; 9] = [
    "SHEET",
    "VIEWS",
    "HIDDEN",
    "SECTION",
    "DIMENSIONS",
    "LABELS",
    "TABLES",
    "IMAGES",
    "LINES",
];

/// AutoCAD Color Index for a primitive's colour: black-ish ink is 7 (black on white, white
/// on black), mid grey 8, light grey 9.
fn aci(c: [u8; 3]) -> u8 {
    let l = (u32::from(c[0]) * 299 + u32::from(c[1]) * 587 + u32::from(c[2]) * 114) / 1000;
    if l < 70 {
        7
    } else if l < 150 {
        8
    } else {
        9
    }
}

/// The nearest standard DXF lineweight (hundredths of a millimetre) to a pen width.
fn lineweight(width_m: f64) -> i32 {
    const W: [i32; 24] = [
        0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
        200, 211,
    ];
    let want = width_m * 1e5;
    *W.iter()
        .min_by(|a, b| {
            (f64::from(**a) - want)
                .abs()
                .total_cmp(&(f64::from(**b) - want).abs())
        })
        .expect("not empty")
}

/// DXF's own codes for the symbols it has (`%%c` diameter, `%%d` degree).
fn dxf_text(s: &str) -> String {
    s.replace('Ø', "%%c").replace('°', "%%d")
}

/// The sheet `r` as DXF text in `units` (see the module docs).
pub fn dxf(r: &SheetRender, units: PaperUnits) -> String {
    let k = match units {
        PaperUnits::Inches => 1.0 / IN,
        PaperUnits::Millimetres => 1000.0,
    };
    let h = r.size[1];
    let x = |p: f64| num(p * k);
    let y = |p: f64| num((h - p) * k);
    // Which layer each primitive goes on: by the item that drew it.
    let mut layer = vec!["SHEET"; r.prims.len()];
    for it in &r.items {
        let l = match it.kind {
            "viewport" => "VIEWS",
            "dimension" => "DIMENSIONS",
            "label" => "LABELS",
            "table" => "TABLES",
            "image" => "IMAGES",
            _ => "LINES",
        };
        for slot in &mut layer[it.prims[0]..it.prims[1].min(r.prims.len())] {
            *slot = l;
        }
    }
    let mut d = Dxf(String::new());
    d.pair(
        999,
        format!("Sawhorse drawing sheet: {}", r.name.replace('\n', " ")),
    );
    d.pair(0, "SECTION");
    d.pair(2, "HEADER");
    d.pair(9, "$INSUNITS");
    d.pair(70, if units == PaperUnits::Inches { 1 } else { 4 });
    d.pair(9, "$MEASUREMENT");
    d.pair(70, if units == PaperUnits::Inches { 0 } else { 1 });
    d.pair(9, "$EXTMIN");
    d.pair(10, "0");
    d.pair(20, "0");
    d.pair(30, "0");
    d.pair(9, "$EXTMAX");
    d.pair(10, x(r.size[0]));
    d.pair(20, num(h * k));
    d.pair(30, "0");
    d.pair(0, "ENDSEC");
    // Linetypes and layers.
    let dash = if units == PaperUnits::Inches {
        (0.06, 0.04)
    } else {
        (1.5, 1.0)
    };
    d.pair(0, "SECTION");
    d.pair(2, "TABLES");
    d.pair(0, "TABLE");
    d.pair(2, "LTYPE");
    d.pair(70, 2);
    d.pair(0, "LTYPE");
    d.pair(2, "CONTINUOUS");
    d.pair(70, 0);
    d.pair(3, "Solid line");
    d.pair(72, 65);
    d.pair(73, 0);
    d.pair(40, "0.0");
    d.pair(0, "LTYPE");
    d.pair(2, "DASHED");
    d.pair(70, 0);
    d.pair(3, "Hidden lines __ __ __");
    d.pair(72, 65);
    d.pair(73, 2);
    d.pair(40, num(dash.0 + dash.1));
    d.pair(49, num(dash.0));
    d.pair(49, num(-dash.1));
    d.pair(0, "ENDTAB");
    d.pair(0, "TABLE");
    d.pair(2, "LAYER");
    d.pair(70, LAYERS.len() + 1);
    for name in std::iter::once("0").chain(LAYERS) {
        d.pair(0, "LAYER");
        d.pair(2, name);
        d.pair(70, 0);
        d.pair(
            62,
            if name == "HIDDEN" || name == "SECTION" {
                8
            } else {
                7
            },
        );
        d.pair(
            6,
            if name == "HIDDEN" {
                "DASHED"
            } else {
                "CONTINUOUS"
            },
        );
    }
    d.pair(0, "ENDTAB");
    d.pair(0, "ENDSEC");

    d.pair(0, "SECTION");
    d.pair(2, "ENTITIES");
    let poly = |d: &mut Dxf, layer: &str, pts: &[[f64; 2]], color: u8, weight: Option<i32>| {
        d.pair(0, "LWPOLYLINE");
        d.pair(100, "AcDbEntity");
        d.pair(8, layer);
        d.pair(62, color);
        if let Some(w) = weight {
            d.pair(370, w);
        }
        d.pair(100, "AcDbPolyline");
        d.pair(90, pts.len());
        d.pair(70, 1);
        for p in pts {
            d.pair(10, x(p[0]));
            d.pair(20, y(p[1]));
        }
    };
    let rect_pts = |r: &[f64; 4]| {
        [
            [r[0], r[1]],
            [r[0] + r[2], r[1]],
            [r[0] + r[2], r[1] + r[3]],
            [r[0], r[1] + r[3]],
        ]
    };
    for (i, p) in r.prims.iter().enumerate() {
        let l = layer[i];
        match p {
            Prim::Line {
                a,
                b,
                width,
                dash,
                color,
            } => {
                let l = if dash.is_some() && l == "VIEWS" {
                    "HIDDEN"
                } else {
                    l
                };
                d.pair(0, "LINE");
                d.pair(100, "AcDbEntity");
                d.pair(8, l);
                if dash.is_some() {
                    d.pair(6, "DASHED");
                }
                d.pair(62, aci(*color));
                d.pair(370, lineweight(*width));
                d.pair(100, "AcDbLine");
                d.pair(10, x(a[0]));
                d.pair(20, y(a[1]));
                d.pair(30, "0");
                d.pair(11, x(b[0]));
                d.pair(21, y(b[1]));
                d.pair(31, "0");
            }
            Prim::Fill { rings, .. } => {
                let l = if l == "VIEWS" { "SECTION" } else { l };
                for ring in rings.iter().filter(|r| r.len() >= 3) {
                    poly(&mut d, l, ring, 8, None);
                }
            }
            Prim::Rect {
                rect,
                width,
                stroke,
                fill,
            } => {
                let color = stroke.or(*fill).map_or(7, aci);
                let weight = stroke.map(|_| lineweight(*width));
                poly(&mut d, l, &rect_pts(rect), color, weight);
            }
            Prim::Image { rect, .. } => poly(&mut d, l, &rect_pts(rect), 8, None),
            Prim::Text {
                at,
                text,
                size,
                anchor,
                angle,
                bold,
                max,
                color,
            } => {
                let shown = match max {
                    Some(m) => fit_text(text, *size, *m, *bold),
                    None => text.clone(),
                };
                if shown.trim().is_empty() {
                    continue;
                }
                d.pair(0, "TEXT");
                d.pair(100, "AcDbEntity");
                d.pair(8, l);
                d.pair(62, aci(*color));
                d.pair(100, "AcDbText");
                d.pair(10, x(at[0]));
                d.pair(20, y(at[1]));
                d.pair(30, "0");
                d.pair(40, num(size * k));
                d.pair(1, dxf_text(&shown));
                if *angle != 0.0 {
                    d.pair(50, num(*angle));
                }
                let just = match anchor {
                    Anchor::Start => 0,
                    Anchor::Middle => 1,
                    Anchor::End => 2,
                };
                if just != 0 {
                    d.pair(72, just);
                    d.pair(11, x(at[0]));
                    d.pair(21, y(at[1]));
                    d.pair(31, "0");
                }
                d.pair(100, "AcDbText");
            }
        }
    }
    d.pair(0, "ENDSEC");
    d.pair(0, "EOF");
    d.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_colours_and_symbols() {
        assert_eq!(lineweight(0.25e-3), 25);
        assert_eq!(lineweight(0.7e-3), 70);
        assert_eq!(lineweight(0.18e-3), 18);
        assert_eq!(aci([24, 26, 25]), 7);
        assert_eq!(aci([110, 116, 112]), 8);
        assert_eq!(aci([170, 174, 170]), 9);
        assert_eq!(dxf_text("Ø 1 1/2\" at 45°"), "%%c 1 1/2\" at 45%%d");
    }
}
