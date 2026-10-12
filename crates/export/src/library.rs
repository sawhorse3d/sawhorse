//! The built-in material library (lane 10): twelve wood species and eight sheet goods with
//! procedural textures drawn here, sixteen flat colours, and glass and metal basics.
//!
//! Every texture is our own: drawn by the code below from value noise on a periodic integer
//! lattice (so each image tiles), with plain `+ - * /` arithmetic, so the bytes are the same
//! natively and in WASM. No image comes from anywhere else. The colours and sizes were chosen
//! by eye for a woodworker's model, not taken from any product.
//!
//! [`ops`] gives what puts a library material into a model: the texture image
//! ([`Op::AddTexture`]) and the material ([`Op::DefineMaterial`]), with its stock link so the
//! cut list reads the species.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use sawhorse_kernel::Op;
use sawhorse_kernel::looks::{StockKind, StockLink};

const M_PER_IN: f64 = 0.0254;

/// How a library texture is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pattern {
    /// Board grain: rings across the image (whole number per image height, so it tiles),
    /// warped by `figure`, with pores.
    Wood {
        light: [u8; 3],
        dark: [u8; 3],
        rings: u32,
        figure: f64,
        pores: f64,
        seed: u32,
    },
    /// A fine even speckle (MDF, hardboard, melamine).
    Speckle {
        base: [u8; 3],
        spread: f64,
        grain: f64,
        seed: u32,
    },
    /// Chips (particle board).
    Chips {
        base: [u8; 3],
        dark: [u8; 3],
        seed: u32,
    },
    /// Strands laid mostly along the length (OSB).
    Strands {
        base: [u8; 3],
        dark: [u8; 3],
        seed: u32,
    },
}

/// One library material.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryMaterial {
    pub name: &'static str,
    /// "Wood", "Sheet goods", "Colours", "Glass and metal".
    pub category: &'static str,
    /// Flat colour (sRGB + alpha); for a textured material, the texture's average.
    pub color: [u8; 4],
    pub pattern: Option<Pattern>,
    /// Real-world size of one texture image, inches (width along the grain).
    pub size_in: [f64; 2],
    pub stock: Option<(StockKind, &'static str)>,
}

/// Pixel size of every library texture.
pub const TEXTURE_PX: (u32, u32) = (512, 256);

fn wood(
    name: &'static str,
    light: [u8; 3],
    dark: [u8; 3],
    rings: u32,
    figure: f64,
    pores: f64,
    seed: u32,
) -> LibraryMaterial {
    LibraryMaterial {
        name,
        category: "Wood",
        color: [0, 0, 0, 255],
        pattern: Some(Pattern::Wood {
            light,
            dark,
            rings,
            figure,
            pores,
            seed,
        }),
        size_in: [24.0, 12.0],
        stock: Some((StockKind::Lumber, name)),
    }
}

fn sheet(name: &'static str, pattern: Pattern, size_in: [f64; 2]) -> LibraryMaterial {
    LibraryMaterial {
        name,
        category: "Sheet goods",
        color: [0, 0, 0, 255],
        pattern: Some(pattern),
        size_in,
        stock: Some((StockKind::Sheet, name)),
    }
}

fn flat(name: &'static str, category: &'static str, color: [u8; 4]) -> LibraryMaterial {
    LibraryMaterial {
        name,
        category,
        color,
        pattern: None,
        size_in: [12.0, 12.0],
        stock: None,
    }
}

fn build() -> Vec<LibraryMaterial> {
    let mut v = vec![
        wood(
            "White oak",
            [205, 178, 136],
            [150, 116, 76],
            9,
            0.9,
            0.55,
            11,
        ),
        wood("Red oak", [214, 168, 128], [160, 100, 70], 8, 1.0, 0.6, 12),
        wood("Walnut", [128, 90, 62], [70, 46, 32], 10, 0.8, 0.35, 13),
        wood("Cherry", [196, 128, 88], [150, 86, 56], 12, 0.6, 0.15, 14),
        wood(
            "Hard maple",
            [236, 214, 178],
            [204, 176, 136],
            14,
            0.5,
            0.08,
            15,
        ),
        wood("Ash", [222, 200, 160], [168, 138, 96], 7, 1.0, 0.5, 16),
        wood("Birch", [232, 208, 170], [196, 164, 120], 12, 0.6, 0.1, 17),
        wood("Poplar", [206, 198, 156], [150, 150, 110], 9, 0.5, 0.1, 18),
        wood("Pine", [232, 196, 140], [186, 128, 70], 6, 0.9, 0.05, 19),
        wood(
            "Douglas fir",
            [222, 172, 124],
            [168, 104, 64],
            7,
            0.7,
            0.05,
            20,
        ),
        wood("Mahogany", [168, 92, 62], [120, 56, 38], 11, 0.6, 0.3, 21),
        wood(
            "Western red cedar",
            [196, 128, 92],
            [140, 78, 54],
            8,
            0.8,
            0.1,
            22,
        ),
    ];
    let ply = |light, dark, seed| Pattern::Wood {
        light,
        dark,
        rings: 4,
        figure: 2.2,
        pores: 0.1,
        seed,
    };
    v.extend([
        sheet(
            "Baltic birch ply",
            ply([236, 214, 178], [214, 186, 146], 31),
            [48.0, 24.0],
        ),
        sheet(
            "Maple ply",
            ply([238, 220, 186], [214, 192, 152], 32),
            [48.0, 24.0],
        ),
        sheet(
            "Red oak ply",
            ply([214, 170, 130], [170, 116, 82], 33),
            [48.0, 24.0],
        ),
        sheet(
            "MDF",
            Pattern::Speckle {
                base: [186, 150, 112],
                spread: 0.07,
                grain: 0.6,
                seed: 34,
            },
            [24.0, 12.0],
        ),
        sheet(
            "Hardboard",
            Pattern::Speckle {
                base: [120, 82, 52],
                spread: 0.08,
                grain: 0.5,
                seed: 35,
            },
            [24.0, 12.0],
        ),
        sheet(
            "White melamine",
            Pattern::Speckle {
                base: [240, 240, 236],
                spread: 0.015,
                grain: 0.9,
                seed: 36,
            },
            [24.0, 12.0],
        ),
        sheet(
            "OSB",
            Pattern::Strands {
                base: [214, 172, 112],
                dark: [150, 108, 62],
                seed: 37,
            },
            [48.0, 24.0],
        ),
        sheet(
            "Particle board",
            Pattern::Chips {
                base: [204, 174, 128],
                dark: [150, 116, 76],
                seed: 38,
            },
            [24.0, 12.0],
        ),
    ]);
    for (name, c) in [
        ("White", [244, 244, 240]),
        ("Off white", [232, 226, 210]),
        ("Cream", [240, 228, 196]),
        ("Grey", [150, 152, 150]),
        ("Charcoal", [62, 64, 66]),
        ("Black", [28, 28, 30]),
        ("Navy", [36, 52, 86]),
        ("Sky blue", [140, 182, 214]),
        ("Teal", [40, 120, 122]),
        ("Sage", [156, 170, 140]),
        ("Forest green", [44, 86, 56]),
        ("Mustard", [214, 170, 50]),
        ("Barn red", [124, 38, 32]),
        ("Brick", [170, 84, 62]),
        ("Tan", [204, 178, 140]),
        ("Chocolate", [86, 56, 40]),
    ] {
        v.push(flat(name, "Colours", [c[0], c[1], c[2], 255]));
    }
    for (name, c) in [
        ("Clear glass", [196, 222, 228, 80]),
        ("Smoked glass", [70, 80, 86, 150]),
        ("Aluminium", [190, 194, 198, 255]),
        ("Steel", [138, 144, 150, 255]),
        ("Black steel", [44, 46, 50, 255]),
        ("Brass", [182, 150, 80, 255]),
        ("Copper", [184, 114, 52, 255]),
    ] {
        v.push(flat(name, "Glass and metal", c));
    }
    v
}

/// The library, in pane order, with each textured material's colour set to its texture's
/// average.
pub fn materials() -> &'static [LibraryMaterial] {
    static LIB: OnceLock<Vec<LibraryMaterial>> = OnceLock::new();
    LIB.get_or_init(|| {
        let mut v = build();
        for m in &mut v {
            if let Some(p) = m.pattern {
                // A coarse sample is enough for the average (and keeps listing cheap).
                let px = pixels_sized(&p, 128, 64);
                let n = (px.len() / 3) as u64;
                let mut sum = [0u64; 3];
                for c in px.chunks(3) {
                    for k in 0..3 {
                        sum[k] += u64::from(c[k]);
                    }
                }
                m.color = [
                    (sum[0] / n) as u8,
                    (sum[1] / n) as u8,
                    (sum[2] / n) as u8,
                    255,
                ];
            }
        }
        v
    })
}

/// A library material by name (case-insensitive).
pub fn find(name: &str) -> Option<&'static LibraryMaterial> {
    materials()
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name.trim()))
}

/// The texture's path inside a model (`assets/lib-<name>.png`).
pub fn texture_path(m: &LibraryMaterial) -> Option<String> {
    m.pattern?;
    let slug: String = m
        .name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    Some(format!("assets/lib-{slug}.png"))
}

/// Library textures drawn so far, by material name.
type PngCache = Mutex<BTreeMap<&'static str, Arc<Vec<u8>>>>;

/// The PNG of a library material's texture (cached after the first call).
pub fn texture_png(m: &LibraryMaterial) -> Option<Arc<Vec<u8>>> {
    static CACHE: OnceLock<PngCache> = OnceLock::new();
    let p = m.pattern?;
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut c = cache.lock().ok()?;
    let png = c
        .entry(m.name)
        .or_insert_with(|| {
            Arc::new(crate::png::encode(
                TEXTURE_PX.0,
                TEXTURE_PX.1,
                3,
                &pixels(&p),
            ))
        })
        .clone();
    Some(png)
}

/// What puts a library material into a model: its texture image (when it has one) and its
/// definition, under `name` (default: the library name).
pub fn ops(m: &LibraryMaterial, name: Option<&str>) -> Vec<Op> {
    let mut out = Vec::new();
    let texture = texture_path(m);
    if let (Some(path), Some(png)) = (&texture, texture_png(m)) {
        out.push(Op::AddTexture {
            path: path.clone(),
            mime: "image/png".into(),
            data: png.as_ref().clone(),
        });
    }
    out.push(Op::DefineMaterial {
        name: name.unwrap_or(m.name).to_string(),
        color: m.color,
        texture,
        size: m
            .pattern
            .map(|_| [m.size_in[0] * M_PER_IN, m.size_in[1] * M_PER_IN]),
        rotation: 0.0,
        offset: [0.0, 0.0],
        stock: m.stock.map(|(kind, species)| StockLink {
            kind,
            species: species.to_string(),
        }),
    });
    out
}

/// One entry as the Materials pane lists it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LibraryEntry {
    pub name: &'static str,
    pub category: &'static str,
    pub color: [u8; 4],
    pub texture: Option<String>,
    /// Texture size, metres.
    pub size: Option<[f64; 2]>,
    pub stock: Option<StockLink>,
}

pub fn entries() -> Vec<LibraryEntry> {
    materials()
        .iter()
        .map(|m| LibraryEntry {
            name: m.name,
            category: m.category,
            color: m.color,
            texture: texture_path(m),
            size: m
                .pattern
                .map(|_| [m.size_in[0] * M_PER_IN, m.size_in[1] * M_PER_IN]),
            stock: m.stock.map(|(kind, species)| StockLink {
                kind,
                species: species.to_string(),
            }),
        })
        .collect()
}

// ---- drawing ------------------------------------------------------------------------------

fn hash(x: i64, y: i64, seed: u32) -> f64 {
    // A 32-bit integer mix (no floating point until the end), so every platform agrees.
    let mut h = (x as u32)
        .wrapping_mul(0x8da6_b343)
        .wrapping_add((y as u32).wrapping_mul(0xd816_3841))
        .wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    f64::from(h) / 4_294_967_296.0
}

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise on a lattice of period (`px`, `py`): tiles exactly.
fn noise(x: f64, y: f64, px: i64, py: i64, seed: u32) -> f64 {
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = (smooth(x - fx), smooth(y - fy));
    let (ix, iy) = (fx as i64, fy as i64);
    let at = |a: i64, b: i64| hash(a.rem_euclid(px), b.rem_euclid(py), seed);
    let a = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * tx;
    let b = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * tx;
    a + (b - a) * ty
}

/// Fractal sum of `octaves` periodic noises starting at lattice period (`px`, `py`), in 0..1.
fn fbm(x: f64, y: f64, px: i64, py: i64, octaves: u32, seed: u32) -> f64 {
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    let mut k = 1i64;
    for o in 0..octaves {
        sum += amp * noise(x * k as f64, y * k as f64, px * k, py * k, seed + o * 101);
        norm += amp;
        amp *= 0.5;
        k *= 2;
    }
    sum / norm
}

fn lerp3(a: [u8; 3], b: [u8; 3], t: f64) -> [f64; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| f64::from(a[i]) + (f64::from(b[i]) - f64::from(a[i])) * t)
}

fn put(px: &mut Vec<u8>, c: [f64; 3], k: f64) {
    for v in c {
        px.push((v * k).round().clamp(0.0, 255.0) as u8);
    }
}

/// The swatch of a library material's texture: the whole image, small (96 x 48), cheap to
/// draw, for the Materials pane.
pub fn swatch_png(m: &LibraryMaterial) -> Option<Vec<u8>> {
    let p = m.pattern?;
    Some(crate::png::encode(96, 48, 3, &pixels_sized(&p, 96, 48)))
}

/// RGB pixels of a pattern at the texture size, rows top to bottom.
pub fn pixels(p: &Pattern) -> Vec<u8> {
    let (w, h) = TEXTURE_PX;
    pixels_sized(p, w, h)
}

/// RGB pixels of a pattern drawn `w` x `h` (the same image at another resolution).
pub fn pixels_sized(p: &Pattern, w: u32, h: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    for j in 0..h {
        for i in 0..w {
            // x runs along the length (the grain), y across; both 0..1 over the image.
            let x = f64::from(i) / f64::from(w);
            let y = f64::from(j) / f64::from(h);
            match *p {
                Pattern::Wood {
                    light,
                    dark,
                    rings,
                    figure,
                    pores,
                    seed,
                } => {
                    let warp = fbm(x * 3.0, y * 2.0, 3, 2, 3, seed) - 0.5;
                    let slow = fbm(x * 2.0, y * 1.0, 2, 1, 2, seed + 7) - 0.5;
                    let t = y * f64::from(rings) + warp * figure + slow * 0.6;
                    let s = t - t.floor();
                    // Earlywood light, latewood dark: a soft band ending each ring.
                    let late = smooth(((s - 0.55) / 0.45).clamp(0.0, 1.0));
                    let early = 1.0 - smooth((s / 0.2).clamp(0.0, 1.0));
                    let band = (late * 0.85 + early * 0.35).min(1.0);
                    let fine = fbm(x * 64.0, y * 8.0, 64, 8, 2, seed + 13);
                    let mut c = lerp3(light, dark, band * 0.75 + (fine - 0.5) * 0.25);
                    // Pores: short dark streaks along the grain.
                    let pore = noise(x * 256.0, y * 96.0, 256, 96, seed + 29);
                    if pore > 1.0 - pores * 0.25 {
                        c = c.map(|v| v * 0.82);
                    }
                    let shade = 0.94 + 0.12 * fbm(x * 4.0, y * 4.0, 4, 4, 2, seed + 41);
                    put(&mut px, c, shade);
                }
                Pattern::Speckle {
                    base,
                    spread,
                    grain,
                    seed,
                } => {
                    let n = noise(x * 256.0, y * 128.0, 256, 128, seed) * grain
                        + fbm(x * 16.0, y * 8.0, 16, 8, 3, seed + 3) * (1.0 - grain);
                    let k = 1.0 + (n - 0.5) * 2.0 * spread;
                    put(&mut px, base.map(f64::from), k);
                }
                Pattern::Chips { base, dark, seed } => {
                    let n = noise(x * 96.0, y * 48.0, 96, 48, seed);
                    let m = noise(x * 192.0, y * 96.0, 192, 96, seed + 5);
                    let t = if n > 0.72 {
                        0.8
                    } else if m > 0.8 {
                        0.45
                    } else {
                        n * 0.3
                    };
                    put(&mut px, lerp3(base, dark, t), 1.0);
                }
                Pattern::Strands { base, dark, seed } => {
                    // Long flakes: noise stretched along x, quantised into patches.
                    let a = noise(x * 12.0, y * 24.0, 12, 24, seed);
                    let b = noise(x * 24.0, y * 48.0, 24, 48, seed + 9);
                    let edge = noise(x * 48.0, y * 96.0, 48, 96, seed + 17);
                    let patch = ((a * 0.6 + b * 0.4) * 6.0).floor() / 6.0;
                    let t = patch * 0.8 + if edge > 0.86 { 0.35 } else { 0.0 };
                    put(&mut px, lerp3(base, dark, t), 1.0);
                }
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starter_set_is_what_the_lane_promised() {
        let m = materials();
        let count = |c: &str| m.iter().filter(|x| x.category == c).count();
        assert_eq!(count("Wood"), 12);
        assert_eq!(count("Sheet goods"), 8);
        assert_eq!(count("Colours"), 16);
        assert!(count("Glass and metal") >= 4);
        let mut names: Vec<&str> = m.iter().map(|x| x.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), m.len(), "names are unique");
        // Woods are lumber with their species; sheet goods are sheets.
        assert_eq!(
            find("white oak").unwrap().stock,
            Some((StockKind::Lumber, "White oak"))
        );
        assert_eq!(
            find("Baltic birch ply").unwrap().stock.map(|s| s.0),
            Some(StockKind::Sheet)
        );
    }

    #[test]
    fn textures_are_pngs_that_tile_and_put_a_material_in_a_model() {
        let oak = find("White oak").unwrap();
        let png = texture_png(oak).unwrap();
        assert_eq!(
            sawhorse_kernel::looks::image_size(&png),
            Some(("image/png", TEXTURE_PX))
        );
        assert!(png.len() < 300_000, "compressed: {} bytes", png.len());
        // Tiling: the last column continues into the first and the last row into the first
        // (the step across the seam is no bigger than the steps inside the image).
        let px = pixels(oak.pattern.as_ref().unwrap());
        let (w, h) = (TEXTURE_PX.0 as usize, TEXTURE_PX.1 as usize);
        let lum = |i: usize, j: usize| {
            let k = (j * w + i) * 3;
            i32::from(px[k]) + i32::from(px[k + 1]) + i32::from(px[k + 2])
        };
        let seam: i32 = (0..h).map(|j| (lum(w - 1, j) - lum(0, j)).abs()).sum();
        let inside: i32 = (0..h)
            .map(|j| (lum(w / 2, j) - lum(w / 2 + 1, j)).abs())
            .sum();
        assert!(
            seam < inside * 3 + h as i32 * 30,
            "seam {seam}, inside {inside}"
        );
        // The average colour is a wood colour, not black.
        assert!(oak.color[0] > 120 && oak.color[0] > oak.color[2]);
        // The ops define it, with its texture held and its stock link.
        let mut model = sawhorse_kernel::Model::new();
        for op in ops(oak, None) {
            model.apply(&op).unwrap();
        }
        let mat = &model.materials()["White oak"];
        assert_eq!(mat.texture.as_deref(), Some("assets/lib-white-oak.png"));
        assert!(model.textures().contains_key("assets/lib-white-oak.png"));
        assert_eq!(mat.stock.as_ref().unwrap().species, "White oak");
        // A flat colour is one op, no texture.
        assert_eq!(ops(find("Navy").unwrap(), None).len(), 1);
    }

    #[test]
    fn drawing_is_deterministic() {
        let p = find("OSB").unwrap().pattern.unwrap();
        assert_eq!(pixels(&p), pixels(&p));
        let h = |d: &[u8]| {
            let mut s = sawhorse_kernel::hash::StableHasher::default();
            s.bytes(d);
            s.finish()
        };
        // Every library texture differs from every other.
        let all: Vec<u64> = materials()
            .iter()
            .filter_map(|m| m.pattern.map(|p| h(&pixels(&p))))
            .collect();
        let mut u = all.clone();
        u.sort();
        u.dedup();
        assert_eq!(u.len(), all.len());
    }
}
