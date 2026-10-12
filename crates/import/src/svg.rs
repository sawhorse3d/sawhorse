//! SVG: every path and basic shape becomes sketch strokes on the ground plane (Z = 0),
//! curves and arcs flattened to short segments. Units come from the root element: its
//! `width`/`height` (with `mm`, `cm`, `in`, `pt`, `pc` or `px`) against its `viewBox`; with
//! no units the CSS pixel (1/96 in) is assumed and noted. SVG's y axis runs down; the
//! drawing is flipped so it reads the same way up on the ground. `transform` attributes
//! (on shapes and groups) are applied. Text, images, `<use>` references and anything in
//! `<defs>`, `<clipPath>`, `<mask>`, `<symbol>`, `<marker>` or `<pattern>` are not
//! geometry and are skipped (counted in the notes).

use std::collections::BTreeMap;

use glam::{DAffine2, DVec2};
use quick_xml::events::{BytesStart, Event};
use sawhorse_kernel::model::ImportStroke;

use crate::{ImportError, ImportOptions, Parsed, malformed};

/// Metres per CSS pixel.
pub const M_PER_PX: f64 = 0.0254 / 96.0;

fn err(m: impl Into<String>) -> ImportError {
    malformed("SVG", m)
}

/// A length with optional unit, in metres (unitless = CSS px), or `None` for percentages.
fn length_m(s: &str) -> Option<f64> {
    let s = s.trim();
    let (num, unit) = s
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .map(|i| (&s[..i], &s[i..]))
        .unwrap_or((s, ""));
    let v: f64 = num.trim().parse().ok()?;
    Some(
        v * match unit.trim() {
            "" | "px" => M_PER_PX,
            "mm" => 0.001,
            "cm" => 0.01,
            "in" => 0.0254,
            "pt" => 0.0254 / 72.0,
            "pc" => 0.0254 / 6.0,
            "m" => 1.0,
            _ => return None,
        },
    )
}

/// Numbers in an SVG attribute (commas, spaces, signs and second dots separate them).
fn numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' {
            let start = i;
            let mut seen_dot = c == b'.';
            let mut seen_e = false;
            i += 1;
            while i < b.len() {
                let d = b[i];
                if d.is_ascii_digit() {
                    i += 1;
                } else if d == b'.' && !seen_dot && !seen_e {
                    seen_dot = true;
                    i += 1;
                } else if (d == b'e' || d == b'E') && !seen_e {
                    seen_e = true;
                    i += 1;
                    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(v) = s[start..i].parse() {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Parse a `transform` attribute.
fn transform(s: &str) -> DAffine2 {
    let mut m = DAffine2::IDENTITY;
    for part in s.split(')') {
        let Some((name, args)) = part.split_once('(') else {
            continue;
        };
        let a = numbers(args);
        let g = |i: usize, d: f64| a.get(i).copied().unwrap_or(d);
        let t = match name.trim().trim_start_matches(',').trim() {
            "matrix" if a.len() == 6 => {
                DAffine2::from_cols_array(&[a[0], a[1], a[2], a[3], a[4], a[5]])
            }
            "translate" => DAffine2::from_translation(DVec2::new(g(0, 0.0), g(1, 0.0))),
            "scale" => DAffine2::from_scale(DVec2::new(g(0, 1.0), g(1, g(0, 1.0)))),
            "rotate" => {
                let c = DVec2::new(g(1, 0.0), g(2, 0.0));
                DAffine2::from_translation(c)
                    * DAffine2::from_angle(g(0, 0.0).to_radians())
                    * DAffine2::from_translation(-c)
            }
            "skewX" => {
                DAffine2::from_cols_array(&[1.0, 0.0, g(0, 0.0).to_radians().tan(), 1.0, 0.0, 0.0])
            }
            "skewY" => {
                DAffine2::from_cols_array(&[1.0, g(0, 0.0).to_radians().tan(), 0.0, 1.0, 0.0, 0.0])
            }
            _ => DAffine2::IDENTITY,
        };
        m *= t;
    }
    m
}

/// Segments for a curve whose control polygon is `len` user units long.
fn segments(len: f64, units_per_mm: f64) -> usize {
    ((len / units_per_mm / 2.0).ceil() as usize).clamp(4, 64)
}

fn cubic(p0: DVec2, p1: DVec2, p2: DVec2, p3: DVec2, upm: f64) -> Vec<DVec2> {
    let n = segments(p0.distance(p1) + p1.distance(p2) + p2.distance(p3), upm);
    (1..=n)
        .map(|k| {
            let t = k as f64 / n as f64;
            let u = 1.0 - t;
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
        })
        .collect()
}

fn quad(p0: DVec2, p1: DVec2, p2: DVec2, upm: f64) -> Vec<DVec2> {
    let n = segments(p0.distance(p1) + p1.distance(p2), upm);
    (1..=n)
        .map(|k| {
            let t = k as f64 / n as f64;
            let u = 1.0 - t;
            p0 * (u * u) + p1 * (2.0 * u * t) + p2 * (t * t)
        })
        .collect()
}

/// SVG elliptical arc (endpoint parameterisation, SVG 1.1 F.6.5) as points after `p0`.
#[allow(clippy::too_many_arguments)]
fn arc(
    p0: DVec2,
    rx: f64,
    ry: f64,
    phi_deg: f64,
    large: bool,
    sweep: bool,
    p1: DVec2,
    upm: f64,
) -> Vec<DVec2> {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-12 || ry < 1e-12 || p0.distance(p1) < 1e-12 {
        return vec![p1];
    }
    let phi = phi_deg.to_radians();
    let (c, s) = (phi.cos(), phi.sin());
    let d = (p0 - p1) / 2.0;
    let x1 = DVec2::new(c * d.x + s * d.y, -s * d.x + c * d.y);
    let lambda = (x1.x * x1.x) / (rx * rx) + (x1.y * x1.y) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * x1.y * x1.y - ry * ry * x1.x * x1.x).max(0.0);
    let den = rx * rx * x1.y * x1.y + ry * ry * x1.x * x1.x;
    let mut co = (num / den).sqrt();
    if large == sweep {
        co = -co;
    }
    let cx1 = DVec2::new(co * rx * x1.y / ry, -co * ry * x1.x / rx);
    let mid = (p0 + p1) / 2.0;
    let centre = DVec2::new(c * cx1.x - s * cx1.y, s * cx1.x + c * cx1.y) + mid;
    let ang = |u: DVec2, v: DVec2| {
        let a = u.angle_to(v);
        if a.is_nan() { 0.0 } else { a }
    };
    let v1 = DVec2::new((x1.x - cx1.x) / rx, (x1.y - cx1.y) / ry);
    let v2 = DVec2::new((-x1.x - cx1.x) / rx, (-x1.y - cx1.y) / ry);
    let theta1 = ang(DVec2::X, v1);
    let mut dtheta = ang(v1, v2);
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f64::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f64::consts::TAU;
    }
    let n = segments(dtheta.abs() * rx.max(ry), upm)
        .max((dtheta.abs() / std::f64::consts::TAU * 48.0).ceil() as usize);
    (1..=n)
        .map(|k| {
            if k == n {
                return p1;
            }
            let t = theta1 + dtheta * k as f64 / n as f64;
            let e = DVec2::new(rx * t.cos(), ry * t.sin());
            centre + DVec2::new(c * e.x - s * e.y, s * e.x + c * e.y)
        })
        .collect()
}

/// Sub-paths of a path `d` (user units), each with its closed flag.
fn path(d: &str, upm: f64) -> Vec<(Vec<DVec2>, bool)> {
    let mut out: Vec<(Vec<DVec2>, bool)> = Vec::new();
    let mut cur: Vec<DVec2> = Vec::new();
    let mut pen = DVec2::ZERO;
    let mut start = DVec2::ZERO;
    let mut last_ctrl: Option<(char, DVec2)> = None;
    // Split into (command, numbers).
    let mut cmds: Vec<(char, Vec<f64>)> = Vec::new();
    let mut buf = String::new();
    let mut cmd: Option<char> = None;
    for ch in d.chars() {
        if "MmLlHhVvCcSsQqTtAaZz".contains(ch) {
            if let Some(c) = cmd {
                cmds.push((c, numbers(&buf)));
            }
            cmd = Some(ch);
            buf.clear();
        } else {
            buf.push(ch);
        }
    }
    if let Some(c) = cmd {
        cmds.push((c, numbers(&buf)));
    }
    let flush = |cur: &mut Vec<DVec2>, out: &mut Vec<(Vec<DVec2>, bool)>, closed: bool| {
        if cur.len() >= 2 {
            out.push((std::mem::take(cur), closed));
        } else {
            cur.clear();
        }
    };
    for (c, n) in cmds {
        let rel = c.is_ascii_lowercase();
        let base = |pen: DVec2| if rel { pen } else { DVec2::ZERO };
        match c.to_ascii_uppercase() {
            'M' => {
                flush(&mut cur, &mut out, false);
                for (k, p) in n.as_chunks::<2>().0.iter().enumerate() {
                    pen = base(pen) + DVec2::new(p[0], p[1]);
                    if k == 0 {
                        start = pen;
                    }
                    cur.push(pen);
                }
                last_ctrl = None;
            }
            'L' => {
                for p in n.as_chunks::<2>().0.iter() {
                    pen = base(pen) + DVec2::new(p[0], p[1]);
                    cur.push(pen);
                }
                last_ctrl = None;
            }
            'H' => {
                for &x in &n {
                    pen = DVec2::new(if rel { pen.x + x } else { x }, pen.y);
                    cur.push(pen);
                }
                last_ctrl = None;
            }
            'V' => {
                for &y in &n {
                    pen = DVec2::new(pen.x, if rel { pen.y + y } else { y });
                    cur.push(pen);
                }
                last_ctrl = None;
            }
            'C' | 'S' => {
                let k = if c.eq_ignore_ascii_case(&'C') { 6 } else { 4 };
                for p in n.chunks_exact(k) {
                    if cur.is_empty() {
                        cur.push(pen);
                    }
                    let b = base(pen);
                    let (c1, c2, e) = if k == 6 {
                        (
                            b + DVec2::new(p[0], p[1]),
                            b + DVec2::new(p[2], p[3]),
                            b + DVec2::new(p[4], p[5]),
                        )
                    } else {
                        let c1 = match last_ctrl {
                            Some(('C', q)) => pen * 2.0 - q,
                            _ => pen,
                        };
                        (c1, b + DVec2::new(p[0], p[1]), b + DVec2::new(p[2], p[3]))
                    };
                    cur.extend(cubic(pen, c1, c2, e, upm));
                    last_ctrl = Some(('C', c2));
                    pen = e;
                }
            }
            'Q' | 'T' => {
                let k = if c.eq_ignore_ascii_case(&'Q') { 4 } else { 2 };
                for p in n.chunks_exact(k) {
                    if cur.is_empty() {
                        cur.push(pen);
                    }
                    let b = base(pen);
                    let (c1, e) = if k == 4 {
                        (b + DVec2::new(p[0], p[1]), b + DVec2::new(p[2], p[3]))
                    } else {
                        let c1 = match last_ctrl {
                            Some(('Q', q)) => pen * 2.0 - q,
                            _ => pen,
                        };
                        (c1, b + DVec2::new(p[0], p[1]))
                    };
                    cur.extend(quad(pen, c1, e, upm));
                    last_ctrl = Some(('Q', c1));
                    pen = e;
                }
            }
            'A' => {
                for p in n.as_chunks::<7>().0.iter() {
                    if cur.is_empty() {
                        cur.push(pen);
                    }
                    let e = base(pen) + DVec2::new(p[5], p[6]);
                    cur.extend(arc(pen, p[0], p[1], p[2], p[3] != 0.0, p[4] != 0.0, e, upm));
                    pen = e;
                }
                last_ctrl = None;
            }
            'Z' => {
                if cur.len() > 1 && cur.last().is_some_and(|l| l.distance(start) < 1e-9) {
                    cur.pop();
                }
                flush(&mut cur, &mut out, true);
                pen = start;
                last_ctrl = None;
            }
            _ => {}
        }
    }
    flush(&mut cur, &mut out, false);
    out
}

fn ellipse_pts(c: DVec2, rx: f64, ry: f64, upm: f64) -> Vec<DVec2> {
    let n = segments(std::f64::consts::TAU * rx.max(ry), upm).max(48);
    (0..n)
        .map(|k| {
            let t = std::f64::consts::TAU * k as f64 / n as f64;
            c + DVec2::new(rx * t.cos(), ry * t.sin())
        })
        .collect()
}

fn attrs(e: &BytesStart) -> BTreeMap<String, String> {
    e.attributes()
        .flatten()
        .filter_map(|a| {
            let k = String::from_utf8_lossy(a.key.local_name().as_ref()).to_string();
            let v = a.unescape_value().ok()?.to_string();
            Some((k, v))
        })
        .collect()
}

pub fn read(bytes: &[u8], _opts: &ImportOptions) -> Result<Parsed, ImportError> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut stack: Vec<(DAffine2, bool)> = vec![(DAffine2::IDENTITY, false)];
    // User units -> metres, and the flip, from the root element.
    let mut to_m = DAffine2::IDENTITY;
    let mut m_per_unit = M_PER_PX;
    let mut flip_known = false;
    let mut root_seen = false;
    let mut shapes: Vec<(Vec<DVec2>, bool)> = Vec::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut notes = Vec::new();
    let mut buf = Vec::new();
    loop {
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| err(e.to_string()))?;
        let (e, empty) = match &ev {
            Event::Start(e) => (e.clone(), false),
            Event::Empty(e) => (e.clone(), true),
            Event::End(_) => {
                if stack.len() > 1 {
                    stack.pop();
                }
                buf.clear();
                continue;
            }
            Event::Eof => break,
            _ => {
                buf.clear();
                continue;
            }
        };
        let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
        let a = attrs(&e);
        let (parent, hidden_parent) = *stack.last().unwrap_or(&(DAffine2::IDENTITY, false));
        let local = a
            .get("transform")
            .map(|t| transform(t))
            .unwrap_or(DAffine2::IDENTITY);
        let m = parent * local;
        let hidden = hidden_parent
            || matches!(
                name.as_str(),
                "defs"
                    | "clipPath"
                    | "mask"
                    | "symbol"
                    | "marker"
                    | "pattern"
                    | "metadata"
                    | "title"
                    | "desc"
                    | "style"
            )
            || a.get("display").is_some_and(|d| d.trim() == "none");
        if !empty {
            stack.push((m, hidden));
        }
        if name == "svg" && !root_seen {
            root_seen = true;
            let vb: Vec<f64> = a.get("viewBox").map(|v| numbers(v)).unwrap_or_default();
            let w = a.get("width").and_then(|v| length_m(v));
            let h = a.get("height").and_then(|v| length_m(v));
            match (vb.len() == 4 && vb[2] > 0.0 && vb[3] > 0.0, w, h) {
                (true, Some(w), _) => {
                    m_per_unit = w / vb[2];
                    if a.get("width").is_some_and(|v| numbers(v).len() == 1 && v.trim().chars().last().is_some_and(|c| c.is_ascii_digit())) {
                        notes.push("SVG: the width has no unit; CSS pixels (1/96 in) assumed".to_string());
                    }
                }
                (true, None, Some(h)) => m_per_unit = h / vb[3],
                (true, None, None) => notes.push("SVG: no width or height; one user unit read as one CSS pixel (1/96 in)".to_string()),
                (false, Some(_), _) | (false, None, Some(_)) => {}
                (false, None, None) => notes.push("SVG: no width, height or viewBox; one user unit read as one CSS pixel (1/96 in)".to_string()),
            }
            if vb.len() == 4 && vb[3] > 0.0 {
                // x' = (x - minx) * s, y' = (miny + h - y) * s: the drawing sits in the positive quadrant, upright.
                to_m = DAffine2::from_cols_array(&[
                    m_per_unit,
                    0.0,
                    0.0,
                    -m_per_unit,
                    -vb[0] * m_per_unit,
                    (vb[1] + vb[3]) * m_per_unit,
                ]);
                flip_known = true;
            } else if let Some(h) = h {
                to_m = DAffine2::from_cols_array(&[m_per_unit, 0.0, 0.0, -m_per_unit, 0.0, h]);
                flip_known = true;
            } else {
                to_m = DAffine2::from_cols_array(&[m_per_unit, 0.0, 0.0, -m_per_unit, 0.0, 0.0]);
            }
            buf.clear();
            continue;
        }
        if hidden {
            buf.clear();
            continue;
        }
        let upm = 0.001 / m_per_unit; // user units per millimetre
        let num = |k: &str| {
            a.get(k)
                .and_then(|v| numbers(v).first().copied())
                .unwrap_or(0.0)
        };
        let mut add = |pts: Vec<DVec2>, closed: bool| {
            let pts: Vec<DVec2> = pts.into_iter().map(|p| m.transform_point2(p)).collect();
            shapes.push((pts, closed));
        };
        match name.as_str() {
            "path" => {
                for (pts, closed) in path(a.get("d").map(String::as_str).unwrap_or(""), upm) {
                    add(pts, closed);
                }
            }
            "line" => add(
                vec![
                    DVec2::new(num("x1"), num("y1")),
                    DVec2::new(num("x2"), num("y2")),
                ],
                false,
            ),
            "polyline" | "polygon" => {
                let v = numbers(a.get("points").map(String::as_str).unwrap_or(""));
                let pts: Vec<DVec2> = v
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| DVec2::new(p[0], p[1]))
                    .collect();
                add(pts, name == "polygon");
            }
            "rect" => {
                let (x, y, w, h) = (num("x"), num("y"), num("width"), num("height"));
                let (mut rx, mut ry) = (num("rx"), num("ry"));
                if a.contains_key("rx") && !a.contains_key("ry") {
                    ry = rx;
                }
                if a.contains_key("ry") && !a.contains_key("rx") {
                    rx = ry;
                }
                let (rx, ry) = (rx.min(w / 2.0), ry.min(h / 2.0));
                if w > 0.0 && h > 0.0 {
                    if rx > 0.0 && ry > 0.0 {
                        let d = format!(
                            "M{} {}H{}A{rx} {ry} 0 0 1 {} {}V{}A{rx} {ry} 0 0 1 {} {}H{}A{rx} {ry} 0 0 1 {} {}V{}A{rx} {ry} 0 0 1 {} {}Z",
                            x + rx,
                            y,
                            x + w - rx,
                            x + w,
                            y + ry,
                            y + h - ry,
                            x + w - rx,
                            y + h,
                            x + rx,
                            x,
                            y + h - ry,
                            y + ry,
                            x + rx,
                            y
                        );
                        for (pts, closed) in path(&d, upm) {
                            add(pts, closed);
                        }
                    } else {
                        add(
                            vec![
                                DVec2::new(x, y),
                                DVec2::new(x + w, y),
                                DVec2::new(x + w, y + h),
                                DVec2::new(x, y + h),
                            ],
                            true,
                        );
                    }
                }
            }
            "circle" => {
                let r = num("r");
                if r > 0.0 {
                    add(
                        ellipse_pts(DVec2::new(num("cx"), num("cy")), r, r, upm),
                        true,
                    );
                }
            }
            "ellipse" => {
                let (rx, ry) = (num("rx"), num("ry"));
                if rx > 0.0 && ry > 0.0 {
                    add(
                        ellipse_pts(DVec2::new(num("cx"), num("cy")), rx, ry, upm),
                        true,
                    );
                }
            }
            "g" | "a" | "switch" => {}
            "text" | "image" | "use" | "foreignObject" => {
                *skipped.entry(format!("<{name}>")).or_default() += 1
            }
            _ => {}
        }
        buf.clear();
    }
    if !root_seen {
        return Err(err("no <svg> element"));
    }
    let mut out = Parsed::default();
    let mut world: Vec<(Vec<DVec2>, bool)> = shapes
        .into_iter()
        .map(|(pts, closed)| {
            let mut p: Vec<DVec2> = pts.into_iter().map(|q| to_m.transform_point2(q)).collect();
            p.dedup_by(|a, b| a.distance(*b) < 1e-9);
            if closed && p.len() > 2 && p[0].distance(p[p.len() - 1]) < 1e-9 {
                p.pop();
            }
            (p, closed)
        })
        .filter(|(p, _)| p.len() >= 2 && p.iter().all(|q| q.is_finite()))
        .collect();
    if !flip_known {
        // Without a height to flip about, sit the drawing on the X axis.
        let min_y = world
            .iter()
            .flat_map(|(p, _)| p)
            .fold(f64::INFINITY, |m, q| m.min(q.y));
        if min_y.is_finite() {
            for (p, _) in &mut world {
                for q in p {
                    q.y -= min_y;
                }
            }
        }
    }
    for (p, closed) in world {
        out.strokes.push(ImportStroke {
            points: p.iter().map(|q| [q.x, q.y, 0.0]).collect(),
            closed: closed && p.len() > 2,
        });
    }
    out.notes = notes;
    if !skipped.is_empty() {
        let list: Vec<String> = skipped.iter().map(|(k, n)| format!("{n} {k}")).collect();
        out.notes
            .push(format!("SVG: not imported: {}", list.join(", ")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawhorse_kernel::Document;

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
    }

    #[test]
    fn round_trip_through_our_svg_outline() {
        use sawhorse_export::svg as wsvg;
        let sq = |o: f64, s: f64| {
            vec![
                DVec2::new(o, o),
                DVec2::new(o + s, o),
                DVec2::new(o + s, o + s),
                DVec2::new(o, o + s),
            ]
        };
        let text = wsvg::outline(&[sq(0.0, 0.3), sq(0.1, 0.1)]).unwrap();
        let p = read(text.as_bytes(), &ImportOptions::default()).unwrap();
        assert!(p.notes.is_empty(), "{:?}", p.notes);
        assert_eq!(p.strokes.len(), 2);
        assert!(p.strokes[0].closed);
        assert!(
            p.strokes[0]
                .points
                .iter()
                .any(|q| close(*q, [0.3, 0.3, 0.0]))
        );
        assert!(
            p.strokes[1]
                .points
                .iter()
                .any(|q| close(*q, [0.1, 0.1, 0.0])),
            "the hole is where it was"
        );
        let mut d = Document::with_ground_sketch();
        d.apply(&p.op()).unwrap();
        let regions = d.model().stats().regions;
        assert_eq!(regions, 2);
    }

    #[test]
    fn units_transforms_shapes_and_curves() {
        let svg = r#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 200 100">
  <defs><rect width="5" height="5"/></defs>
  <g transform="translate(10,10)">
    <rect x="0" y="0" width="20" height="10"/>
    <circle cx="50" cy="50" r="10" transform="scale(2)"/>
  </g>
  <path d="M0,0 C 10,0 10,10 20,10 s10,10 20,0 q5,-5 10,0 t10,0 a5,5 0 0 1 10,0 z"/>
  <line x1="0" y1="100" x2="200" y2="100"/>
  <polygon points="0,0 10,0 5,8"/>
  <text x="1" y="1">hi</text>
</svg>"#;
        let p = read(svg.as_bytes(), &ImportOptions::default()).unwrap();
        // rect, circle, path, line, polygon (the rect in defs is not geometry).
        assert_eq!(p.strokes.len(), 5);
        // 200 user units = 100 mm: one unit = 0.5 mm. The rect's corner (10,10) user ->
        // x 5 mm, y flipped: (100 - 10) * 0.5 mm = 45 mm.
        assert!(
            close(p.strokes[0].points[0], [0.005, 0.045, 0.0]),
            "{:?}",
            p.strokes[0].points[0]
        );
        // The circle: centre (10 + 100, 10 + 100) user, radius 20 user = 10 mm.
        let c = &p.strokes[1].points;
        let centre = [0.055, (100.0 - 110.0) * 0.0005, 0.0];
        for q in c {
            let r = ((q[0] - centre[0]).powi(2) + (q[1] - centre[1]).powi(2)).sqrt();
            assert!((r - 0.01).abs() < 1e-9);
        }
        // The line along the bottom edge lands on y = 0.
        assert!(close(p.strokes[3].points[0], [0.0, 0.0, 0.0]));
        assert!(p.notes.iter().any(|n| n.contains("1 <text>")));
        assert!(
            p.strokes[2].closed && p.strokes[2].points.len() > 20,
            "curves flattened"
        );
        assert!(read(b"<html/>", &ImportOptions::default()).is_err());
    }

    #[test]
    fn number_lists_and_lengths() {
        assert_eq!(numbers("1.5.5-2e1,3"), vec![1.5, 0.5, -20.0, 3.0]);
        assert_eq!(length_m("1in"), Some(0.0254));
        assert_eq!(length_m("96"), Some(0.0254));
        assert_eq!(length_m("50%"), None);
        let t = transform("rotate(90) translate(1 0)");
        assert!((t.transform_point2(DVec2::ZERO) - DVec2::new(0.0, 1.0)).length() < 1e-12);
    }
}
