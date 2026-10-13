//! Materials, paint, texture positioning, styles and shadows (lane 10, ADR-0008).
//!
//! - A **material** is a name, an sRGB colour with alpha, an optional texture (an image the
//!   model holds, [`TextureImage`], with a real-world size, a rotation and an offset) and an
//!   optional **stock link** ([`StockLink`]: lumber or sheet goods, and the species in
//!   words) that the cut list reads when a part names no stock of its own.
//! - **Paint** goes on faces (a solid's [`FacePaint`], by persistent face id) or on a solid,
//!   group or component instance (its *default*). What a face shows is its own material, else
//!   its solid's, else the nearest enclosing group or instance that has one
//!   ([`Model::face_material`]). Painting a closed container never touches faces that have a
//!   material of their own. Back faces are never painted: they show the style's back colour.
//! - **Texture positioning** ([`UvXform`]): per face, a scale (of the material's size), a
//!   rotation and an offset on top of the material's own. Coordinates come from the face's
//!   stored plane frame, which moves with the solid (moves, copies, component placements), so
//!   a texture stays put on the wood. By default the texture's x axis (the grain of a wood
//!   image) runs along the longer side of each face. Skew and projection are later.
//! - **Styles** ([`Style`]) and **shadows** ([`Shadows`]) are document state, saved in
//!   `.shz` and in scenes (a scene that has them brings them back when activated).
//!
//! Everything here is hashed only when present, so every older document keeps its hash.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{DVec2, DVec3};

use crate::error::{DocumentError, KernelError, StructureError};
use crate::hash::StableHasher;
use crate::ids::{ObjectId, from_public};
use crate::model::{Material, Model, OpOutcome};
use crate::nodes::{Item, MemberKind, Thing};
use crate::object::Object;
use crate::plane::Plane;

/// Largest texture image the model takes, bytes.
pub const MAX_TEXTURE_BYTES: usize = 16 * 1024 * 1024;
/// Most texture images in one model.
pub const MAX_TEXTURES: usize = 512;
/// Texture size used when a material gives none: one foot across.
pub const DEFAULT_TEXTURE_SIZE: f64 = 0.3048;

/// Lumber or sheet goods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum StockKind {
    Lumber,
    Sheet,
}

/// What a material is as stock: lumber or sheet goods, and the species or product in words
/// ("White oak", "Baltic birch ply"). The cut list reads it for parts that name no stock.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct StockLink {
    pub kind: StockKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub species: String,
}

fn one2() -> [f64; 2] {
    [1.0, 1.0]
}

fn is_one2(s: &[f64; 2]) -> bool {
    *s == [1.0, 1.0]
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

fn is_zero2(x: &[f64; 2]) -> bool {
    *x == [0.0, 0.0]
}

/// How a texture sits on one face, on top of its material's own size, rotation and offset:
/// `scale` multiplies the material's size (2 = twice as big), `rotation` turns the texture
/// (degrees, counter-clockwise seen from the face's front), `offset` slides it along the face
/// (metres, in the face's texture frame).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct UvXform {
    #[serde(default = "one2", skip_serializing_if = "is_one2")]
    pub scale: [f64; 2],
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
    #[serde(default, skip_serializing_if = "is_zero2")]
    pub offset: [f64; 2],
}

impl Default for UvXform {
    fn default() -> Self {
        UvXform {
            scale: one2(),
            rotation: 0.0,
            offset: [0.0, 0.0],
        }
    }
}

impl UvXform {
    pub fn is_identity(&self) -> bool {
        *self == UvXform::default()
    }

    /// Finite, scale between 1/1000 and 1000, offset within the model's bounds. Rotation is
    /// brought into [0, 360).
    pub fn checked(&self) -> Result<UvXform, KernelError> {
        let ok = self
            .scale
            .iter()
            .all(|s| s.is_finite() && (1e-3..=1e3).contains(s))
            && self.rotation.is_finite()
            && self.rotation.abs() <= 1e6
            && self
                .offset
                .iter()
                .all(|o| o.is_finite() && o.abs() <= crate::tolerance::MAX_COORD);
        if !ok {
            return Err(LooksError::BadPosition.into());
        }
        Ok(UvXform {
            rotation: norm_deg(self.rotation),
            ..*self
        })
    }
}

fn norm_deg(d: f64) -> f64 {
    let r = d.rem_euclid(360.0);
    // -0.0 and 360.0 (from rounding) read as 0.
    if r == 0.0 || r >= 360.0 { 0.0 } else { r }
}

/// One face's own paint: a material (`None`: the solid's) and how its texture sits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FacePaint {
    pub material: Option<String>,
    pub uv: UvXform,
}

impl FacePaint {
    pub fn is_default(&self) -> bool {
        self.material.is_none() && self.uv.is_identity()
    }
}

/// A texture image the model holds (PNG or JPEG bytes, never decoded by the kernel beyond
/// its size), shared between copies of the model.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureImage {
    pub mime: String,
    pub data: Arc<Vec<u8>>,
    /// Pixel size read from the image header.
    pub width: u32,
    pub height: u32,
    /// FNV-1a 64 of the bytes: what the hash, the worker's texture cache and `.shz` use.
    pub digest: u64,
}

impl TextureImage {
    /// Check the bytes are a PNG or JPEG of the given type and read its pixel size.
    pub fn new(mime: &str, data: Vec<u8>) -> Result<TextureImage, KernelError> {
        if data.is_empty() || data.len() > MAX_TEXTURE_BYTES {
            return Err(LooksError::BadTexture("empty, or larger than 16 MB".into()).into());
        }
        let (want, (width, height)) = match image_size(&data) {
            Some(x) => x,
            None => {
                return Err(LooksError::BadTexture("not a PNG or JPEG image".into()).into());
            }
        };
        if mime != want {
            return Err(LooksError::BadTexture(format!("the bytes are {want}, not {mime}")).into());
        }
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err(LooksError::BadTexture("image size out of range".into()).into());
        }
        let mut h = StableHasher::default();
        h.bytes(&data);
        Ok(TextureImage {
            mime: want.to_string(),
            digest: h.finish(),
            data: Arc::new(data),
            width,
            height,
        })
    }

    pub fn digest_hex(&self) -> String {
        format!("{:016x}", self.digest)
    }
}

/// `image/png` or `image/jpeg` and the pixel size, from the header.
pub fn image_size(d: &[u8]) -> Option<(&'static str, (u32, u32))> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if d.len() >= 24 && d.starts_with(PNG) && &d[12..16] == b"IHDR" {
        let w = u32::from_be_bytes(d[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(d[20..24].try_into().ok()?);
        return Some(("image/png", (w, h)));
    }
    if d.len() >= 4 && d[0] == 0xFF && d[1] == 0xD8 {
        // Walk the markers to a start-of-frame.
        let mut i = 2;
        while i + 4 <= d.len() {
            if d[i] != 0xFF {
                return None;
            }
            let m = d[i + 1];
            if m == 0xFF {
                i += 1;
                continue;
            }
            if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
                i += 2;
                continue;
            }
            let len = usize::from(u16::from_be_bytes([d[i + 2], d[i + 3]]));
            if len < 2 {
                return None;
            }
            let sof = matches!(m, 0xC0..=0xCF) && !matches!(m, 0xC4 | 0xC8 | 0xCC);
            if sof {
                if i + 9 > d.len() {
                    return None;
                }
                let h = u32::from(u16::from_be_bytes([d[i + 5], d[i + 6]]));
                let w = u32::from(u16::from_be_bytes([d[i + 7], d[i + 8]]));
                return Some(("image/jpeg", (w, h)));
            }
            i += 2 + len;
        }
    }
    None
}

/// A texture path: `assets/<name>.png` or `.jpg`, letters, digits, `.`, `_` and `-` only.
pub fn check_texture_path(path: &str) -> Result<(), KernelError> {
    let ok = path.len() <= 96
        && path.strip_prefix("assets/").is_some_and(|n| {
            !n.is_empty()
                && !n.starts_with('.')
                && !n.contains("..")
                && n.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
        && (path.ends_with(".png") || path.ends_with(".jpg") || path.ends_with(".jpeg"));
    if ok {
        Ok(())
    } else {
        Err(LooksError::BadTexture(format!(
            "a texture is named assets/<name>.png or .jpg (letters, digits, . _ -), not {path:?}"
        ))
        .into())
    }
}

/// How faces are drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum FaceStyle {
    /// Materials with their textures (the default).
    #[default]
    Textured,
    /// Material colours, no textures.
    Shaded,
    /// One colour for every face (the theme's), no materials.
    Monochrome,
    /// Faces in the background colour: only the edges you could see.
    HiddenLine,
    /// No faces: every edge, front and back.
    Wireframe,
    /// See-through faces with every edge.
    XRay,
}

impl FaceStyle {
    fn code(self) -> u64 {
        match self {
            FaceStyle::Textured => 0,
            FaceStyle::Shaded => 1,
            FaceStyle::Monochrome => 2,
            FaceStyle::HiddenLine => 3,
            FaceStyle::Wireframe => 4,
            FaceStyle::XRay => 5,
        }
    }
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// How edges are drawn. Jitter is never drawn (the lines are a woodworker's drawing, not a
/// sketch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct EdgeStyle {
    /// Edges drawn at all.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub edges: bool,
    /// Outlines (silhouettes and open boundaries) drawn heavier.
    #[serde(default, skip_serializing_if = "is_false")]
    pub profiles: bool,
    /// Edges run a little past their ends, like pencil lines.
    #[serde(default, skip_serializing_if = "is_false")]
    pub extensions: bool,
    /// The last bit of each edge drawn heavier.
    #[serde(default, skip_serializing_if = "is_false")]
    pub endpoints: bool,
    /// Lane 15: soft and hidden edges drawn too, dashed (SketchUp's "hidden geometry"), so
    /// they can be seen and picked; off, they are left out.
    #[serde(default, skip_serializing_if = "is_false")]
    pub soft: bool,
}

impl Default for EdgeStyle {
    fn default() -> Self {
        EdgeStyle {
            edges: true,
            profiles: false,
            extensions: false,
            endpoints: false,
            soft: false,
        }
    }
}

/// A drawing style: faces, edges and the colours behind the model.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Style {
    /// The preset it started from, for the Styles pane ("Workshop", ...). Informational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default)]
    pub face: FaceStyle,
    #[serde(default)]
    pub edges: EdgeStyle,
    /// Background colour (sRGB); absent: the theme's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<[u8; 3]>,
    /// Sky colour above the horizon; absent: no sky.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sky: Option<[u8; 3]>,
    /// Ground colour below the horizon; absent: no ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<[u8; 3]>,
    /// Active section planes cut the model (lane 12; the switch was kept since lane 10).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub section_cuts: bool,
    /// Section planes drawn (lane 12).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub section_planes: bool,
    /// The cut faces filled (lane 12).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub section_fill: bool,
    /// The section fill colour (sRGB); absent: the theme's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_fill_color: Option<[u8; 3]>,
    /// The cut outline's colour (sRGB); absent: the edge colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_line_color: Option<[u8; 3]>,
    /// The cut outline's width in screen pixels, 1 to 12; absent: 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_line_width: Option<u8>,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            preset: None,
            face: FaceStyle::Textured,
            edges: EdgeStyle::default(),
            background: None,
            sky: None,
            ground: None,
            section_cuts: true,
            section_planes: true,
            section_fill: true,
            section_fill_color: None,
            section_line_color: None,
            section_line_width: None,
        }
    }
}

impl Style {
    fn check(&self) -> Result<(), KernelError> {
        if let Some(p) = &self.preset
            && (p.trim().is_empty() || p.chars().count() > 64)
        {
            return Err(DocumentError::BadName.into());
        }
        if self
            .section_line_width
            .is_some_and(|w| !(1..=12).contains(&w))
        {
            return Err(LooksError::BadStyle(
                "the section line width must be 1 to 12 pixels".into(),
            )
            .into());
        }
        Ok(())
    }

    pub(crate) fn hash_into(&self, h: &mut StableHasher) {
        h.tag("style");
        h.tag(self.preset.as_deref().unwrap_or(""));
        h.u64(self.face.code());
        let e = self.edges;
        h.u64(
            u64::from(e.edges)
                | u64::from(e.profiles) << 1
                | u64::from(e.extensions) << 2
                | u64::from(e.endpoints) << 3
                | u64::from(self.section_cuts) << 4
                // Lane 12's switches count when they are off, so older styles keep their hash.
                | u64::from(!self.section_planes) << 5
                | u64::from(!self.section_fill) << 6
                // Lane 15 (0 when off, so older styles keep their hash).
                | u64::from(e.soft) << 7,
        );
        for c in [self.background, self.sky, self.ground] {
            match c {
                Some(c) => h.bytes(&c),
                None => h.u64(u64::MAX),
            }
        }
        if self.section_fill_color.is_some()
            || self.section_line_color.is_some()
            || self.section_line_width.is_some()
        {
            h.tag("section-style");
            for c in [self.section_fill_color, self.section_line_color] {
                match c {
                    Some(c) => h.bytes(&c),
                    None => h.u64(u64::MAX),
                }
            }
            h.u64(self.section_line_width.map_or(u64::MAX, u64::from));
        }
    }
}

/// The sun and the shadows it casts. Time is local solar time; north is +Y; no geolocation.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Shadows {
    #[serde(default)]
    pub on: bool,
    /// Month 1..=12 and day 1..=31.
    pub month: u8,
    pub day: u8,
    /// Minutes after midnight, 0..1440.
    pub minutes: u16,
    /// Degrees, -90..=90 (north positive).
    pub latitude: f64,
    /// Sunlight and shade brightness, 0..=100.
    pub light: u8,
    pub dark: u8,
    /// Shadows on the ground plane, and on faces.
    #[serde(default = "yes")]
    pub ground: bool,
    #[serde(default = "yes")]
    pub faces: bool,
}

impl Default for Shadows {
    fn default() -> Self {
        Shadows {
            on: false,
            month: 6,
            day: 21,
            minutes: 13 * 60 + 30,
            latitude: 45.0,
            light: 80,
            dark: 45,
            ground: true,
            faces: true,
        }
    }
}

impl Shadows {
    fn check(&self) -> Result<(), KernelError> {
        let days = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let ok = (1..=12).contains(&self.month)
            && self.day >= 1
            && self.day <= days[usize::from(self.month.clamp(1, 12)) - 1]
            && self.minutes < 1440
            && self.latitude.is_finite()
            && (-90.0..=90.0).contains(&self.latitude)
            && self.light <= 100
            && self.dark <= 100;
        if ok {
            Ok(())
        } else {
            Err(LooksError::BadShadows.into())
        }
    }

    pub(crate) fn hash_into(&self, h: &mut StableHasher) {
        h.tag("shadows");
        h.u64(u64::from(self.on) | u64::from(self.ground) << 1 | u64::from(self.faces) << 2);
        h.u64(u64::from(self.month) << 24 | u64::from(self.day) << 16 | u64::from(self.minutes));
        h.u64(fbits(self.latitude));
        h.u64(u64::from(self.light) << 8 | u64::from(self.dark));
    }

    /// Unit vector towards the sun (Z up, north +Y, east +X), by the usual declination and
    /// hour-angle formulas. Below the horizon the z component is negative.
    pub fn sun_direction(&self) -> [f64; 3] {
        let cum = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        let n = f64::from(cum[usize::from(self.month.clamp(1, 12)) - 1] + u16::from(self.day));
        let decl = (23.44f64).to_radians() * ((360.0 / 365.0) * (n - 81.0)).to_radians().sin();
        let hour = (f64::from(self.minutes) / 60.0 - 12.0) * 15.0;
        let (lat, ha) = (self.latitude.to_radians(), hour.to_radians());
        let alt = (lat.sin() * decl.sin() + lat.cos() * decl.cos() * ha.cos())
            .clamp(-1.0, 1.0)
            .asin();
        // Azimuth from north, clockwise (east = 90).
        let az = (-ha.sin() * decl.cos())
            .atan2(lat.cos() * decl.sin() - lat.sin() * decl.cos() * ha.cos());
        [alt.cos() * az.sin(), alt.cos() * az.cos(), alt.sin()]
    }
}

fn fbits(x: f64) -> u64 {
    (x + 0.0).to_bits()
}

/// What a paint op paints.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(tag = "on", rename_all = "snake_case")]
pub enum PaintTarget {
    /// Faces of one solid (persistent face ids).
    Faces { object: u64, faces: Vec<u64> },
    /// Solids, groups and component instances: their default (what their faces without a
    /// material of their own show).
    Items { items: Vec<Item> },
}

/// Refusals from the material ops. Nothing changed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LooksError {
    #[error("a material named {0:?} already exists")]
    MaterialExists(String),
    #[error("no texture {0:?} in the model")]
    NoSuchTexture(String),
    #[error("the texture image cannot be used: {0}")]
    BadTexture(String),
    #[error("a texture size must be between 1/1000 in and 1 km, and rotations and offsets finite")]
    BadMaterial,
    #[error("texture scale must be between 1/1000 and 1000, rotation and offset finite")]
    BadPosition,
    #[error("the shadow settings are out of range (month, day, time, latitude, light, dark)")]
    BadShadows,
    #[error("no face {face} on solid #{object}")]
    NoSuchFace { object: u64, face: u64 },
    #[error("pick at least one face or item to paint")]
    NothingToPaint,
    #[error("the model already holds {MAX_TEXTURES} texture images")]
    TooManyTextures,
    #[error("{0}")]
    BadStyle(String),
}

impl From<LooksError> for KernelError {
    fn from(e: LooksError) -> Self {
        KernelError::Looks(e)
    }
}

/// Base64 (RFC 4648, with padding) for texture bytes in JSON.
pub mod b64 {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(d: &[u8]) -> String {
        let mut s = String::with_capacity(d.len().div_ceil(3) * 4);
        for c in d.chunks(3) {
            let n = (u32::from(c[0]) << 16)
                | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                | u32::from(*c.get(2).unwrap_or(&0));
            s.push(A[(n >> 18) as usize & 63] as char);
            s.push(A[(n >> 12) as usize & 63] as char);
            s.push(if c.len() > 1 {
                A[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            s.push(if c.len() > 2 {
                A[n as usize & 63] as char
            } else {
                '='
            });
        }
        s
    }

    pub fn decode(s: &str) -> Option<Vec<u8>> {
        let s: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if !s.len().is_multiple_of(4) {
            return None;
        }
        let val = |b: u8| -> Option<u32> {
            Some(match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return None,
            } as u32)
        };
        let mut out = Vec::with_capacity(s.len() / 4 * 3);
        for (k, q) in s.chunks(4).enumerate() {
            let last = k + 1 == s.len() / 4;
            let pad = q.iter().rev().take_while(|&&b| b == b'=').count();
            if pad > 2 || (pad > 0 && !last) {
                return None;
            }
            let mut n = 0u32;
            for (i, &b) in q.iter().enumerate() {
                n <<= 6;
                if i < 4 - pad {
                    n |= val(b)?;
                }
            }
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
        }
        Some(out)
    }

    /// `#[serde(with = "crate::looks::b64::serde_bytes")]` for a `Vec<u8>` field.
    pub mod serde_bytes {
        pub fn serialize<S: serde::Serializer>(d: &[u8], s: S) -> Result<S::Ok, S::Error> {
            s.serialize_str(&super::encode(d))
        }
        pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
            let s: String = serde::Deserialize::deserialize(d)?;
            super::decode(&s).ok_or_else(|| serde::de::Error::custom("bad base64"))
        }
    }
}

// ---- texture coordinates ------------------------------------------------------------------

/// A face's texture frame: its stored plane frame, turned a quarter so that the frame's x axis
/// runs along the face's longer side (wood grain along the board). `outer`: the face's outer
/// ring, world points.
pub fn texture_frame(plane: &Plane, outer: &[DVec3]) -> (DVec3, DVec3, DVec3) {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for &p in outer {
        let q = plane.to_local(p);
        lo = lo.min(q);
        hi = hi.max(q);
    }
    let ext = hi - lo;
    // A clear margin so a square face never flips on rounding.
    if ext.y > ext.x * (1.0 + 1e-9) + 1e-12 {
        (plane.origin, plane.v, -plane.u)
    } else {
        (plane.origin, plane.u, plane.v)
    }
}

/// The (u, v) texture coordinate of world point `p` on a face with texture frame `frame`,
/// for a material and the face's own positioning: in repeats of the texture (1.0 = one image
/// across), x along the image's width, y along its height.
pub fn texture_uv(
    frame: &(DVec3, DVec3, DVec3),
    p: DVec3,
    material: &Material,
    texture_px: Option<(u32, u32)>,
    face: &UvXform,
) -> [f64; 2] {
    let (o, u, v) = *frame;
    let d = p - o;
    let q = DVec2::new(d.dot(u), d.dot(v))
        - DVec2::from_array(material.offset)
        - DVec2::from_array(face.offset);
    let th = (material.rotation + face.rotation).to_radians();
    let (s, c) = th.sin_cos();
    let r = DVec2::new(c * q.x + s * q.y, -s * q.x + c * q.y);
    let size = material_size(material, texture_px);
    [
        r.x / (size[0] * face.scale[0]),
        r.y / (size[1] * face.scale[1]),
    ]
}

/// A material's texture size in metres: its own, else one foot across at the image's
/// aspect.
pub fn material_size(m: &Material, texture_px: Option<(u32, u32)>) -> [f64; 2] {
    match (m.size, texture_px) {
        (Some(s), _) => s,
        (None, Some((w, h))) if w > 0 => [
            DEFAULT_TEXTURE_SIZE,
            DEFAULT_TEXTURE_SIZE * f64::from(h) / f64::from(w),
        ],
        _ => [DEFAULT_TEXTURE_SIZE, DEFAULT_TEXTURE_SIZE],
    }
}

/// Fit a face positioning to an affine texture mapping given in world terms (`u = a·p + a0`,
/// `v = b·p + b0`, in repeats): the rotation and the two scales are kept, skew is dropped.
/// `None` when the mapping flattens the face.
pub fn fit_uv(
    frame: &(DVec3, DVec3, DVec3),
    material: &Material,
    texture_px: Option<(u32, u32)>,
    a: [f64; 4],
    b: [f64; 4],
) -> Option<UvXform> {
    let (o, u, v) = *frame;
    let av = DVec3::new(a[0], a[1], a[2]);
    let bv = DVec3::new(b[0], b[1], b[2]);
    // The 2x2 map from the face frame (x along u, y along v) to (u, v) repeats.
    let m = [[av.dot(u), av.dot(v)], [bv.dot(u), bv.dot(v)]];
    let c = [av.dot(o) + a[3], bv.dot(o) + b[3]];
    let (r0, r1) = (DVec2::from_array(m[0]), DVec2::from_array(m[1]));
    let (l0, l1) = (r0.length(), r1.length());
    if !(l0 > 1e-12 && l1 > 1e-12 && l0.is_finite() && l1.is_finite()) {
        return None;
    }
    // Ours: uv = diag(1/W, 1/H) R(-th) (q - off). Row 0 direction is (cos th, sin th).
    let th = r0.y.atan2(r0.x);
    let size = material_size(material, texture_px);
    let (w, h) = (1.0 / l0, 1.0 / l1);
    // Mirrored mappings (row 1 not 90 degrees ahead of row 0) keep their rotation; the
    // mirror itself is skew-like and is dropped.
    let rot_deg = th.to_degrees() - material.rotation;
    // Offset: solve diag(1/W,1/H) R(-th) (-off_total) = c  =>  off_total = -R(th) diag(W,H) c.
    let (s, co) = th.sin_cos();
    let t = DVec2::new(c[0] * w, c[1] * h);
    let off_total = -DVec2::new(co * t.x - s * t.y, s * t.x + co * t.y);
    let off = off_total - DVec2::from_array(material.offset);
    let x = UvXform {
        scale: [w / size[0], h / size[1]],
        rotation: rot_deg,
        offset: off.to_array(),
    };
    x.checked().ok()
}

/// How a texture lies on one face: what the exporters need to write texture coordinates the
/// way the view draws them.
#[derive(Debug, Clone)]
pub struct FaceTexture {
    /// The texture image's path in the model.
    pub path: String,
    frame: (DVec3, DVec3, DVec3),
    material: Material,
    px: Option<(u32, u32)>,
    uv: UvXform,
}

impl FaceTexture {
    /// Texture coordinates of world point `p` on the face (repeats; y along the image's height).
    pub fn uv(&self, p: DVec3) -> [f64; 2] {
        texture_uv(&self.frame, p, &self.material, self.px, &self.uv)
    }
}

// ---- model --------------------------------------------------------------------------------

impl Model {
    /// The material a face shows: its own, else its solid's, else the nearest group or
    /// instance around the solid that has one. `None`: the default look.
    pub fn face_material(&self, object: u64, face: u64) -> Option<&str> {
        let o = self.object(object)?;
        if let Some(m) = o.paint.get(&face).and_then(|p| p.material.as_deref()) {
            return Some(m);
        }
        self.solid_default(o)
    }

    /// What a solid's unpainted faces show: its own material, else the nearest container's.
    pub fn solid_default<'a>(&'a self, o: &'a Object) -> Option<&'a str> {
        o.material()
            .or_else(|| self.container_material(o.member.parent))
    }

    /// The material of the nearest of `parent` and the groups and instances around it that
    /// has one.
    pub(crate) fn container_material(&self, parent: Option<crate::ids::NodeId>) -> Option<&str> {
        let mut p = parent;
        let mut depth = 0;
        while let Some(k) = p {
            let n = self.nodes.get(k)?;
            if let Some(m) = n.material.as_deref() {
                return Some(m);
            }
            p = n.member.parent;
            depth += 1;
            if depth > 256 {
                break;
            }
        }
        None
    }

    /// The texture face `face` of `object` shows, when its material has an image the model
    /// holds.
    pub fn face_texture(&self, object: u64, face: u64) -> Option<FaceTexture> {
        let o = self.object(object)?;
        let b = o.brep();
        let (_, f) = b.faces().find(|(_, f)| f.id == face)?;
        let m = self.materials.get(self.face_material(object, face)?)?;
        let path = m
            .texture
            .clone()
            .filter(|t| self.textures.contains_key(t))?;
        let outer = b.loop_points(f.outer);
        Some(FaceTexture {
            frame: texture_frame(&f.plane, &outer),
            px: self.texture_px(m),
            material: m.clone(),
            uv: o.paint.get(&face).map(|p| p.uv).unwrap_or_default(),
            path,
        })
    }

    /// Paint an imported solid's faces from its item's per-face materials and texture
    /// mappings (`sources`: the item face each solid face came from). Materials the import
    /// did not define are left off, with a note.
    pub(crate) fn import_face_paint(
        &self,
        o: &mut Object,
        item: &crate::meshbuild::ImportItem,
        sources: Option<&Vec<Option<usize>>>,
    ) -> Vec<String> {
        let mut notes = Vec::new();
        let Some(sources) = sources else {
            return notes;
        };
        if item.face_materials.is_empty() && item.face_uvs.is_empty() {
            return notes;
        }
        let ids = o.brep().persistent_face_ids();
        let mut missing = std::collections::BTreeSet::new();
        for (k, src) in sources.iter().enumerate() {
            let (Some(src), Some(&face)) = (src, ids.get(k)) else {
                continue;
            };
            let named = item
                .face_materials
                .get(*src)
                .cloned()
                .flatten()
                .map(|m| {
                    m.trim()
                        .chars()
                        .take(64)
                        .collect::<String>()
                        .trim()
                        .to_string()
                })
                .filter(|m| !m.is_empty());
            let material = match named {
                Some(m) if self.materials.contains_key(&m) => Some(m),
                Some(m) => {
                    missing.insert(m);
                    None
                }
                None => None,
            };
            let mut paint = FacePaint {
                material: material.filter(|m| Some(m.as_str()) != o.material()),
                uv: UvXform::default(),
            };
            let shows = paint.material.as_deref().or(o.material());
            if let (Some(Some([a, b])), Some(m)) = (
                item.face_uvs.get(*src),
                shows.and_then(|n| self.materials.get(n)),
            ) && m
                .texture
                .as_deref()
                .is_some_and(|t| self.textures.contains_key(t))
                && let Some((_, f)) = o.brep().faces().find(|(_, f)| f.id == face)
            {
                let frame = texture_frame(&f.plane, &o.brep().loop_points(f.outer));
                if let Some(uv) = fit_uv(&frame, m, self.texture_px(m), *a, *b) {
                    paint.uv = uv;
                }
            }
            if !paint.is_default() {
                o.paint.insert(face, paint);
            }
        }
        for m in missing {
            notes.push(format!(
                "{}: faces painted with {m:?}, which the file does not define, came in unpainted",
                item.name
            ));
        }
        notes
    }

    /// The Paint tool's sample (Alt+click): the material a face shows.
    pub fn sample(&self, object: u64, face: u64) -> Result<Option<String>, KernelError> {
        let o = self
            .object(object)
            .ok_or(DocumentError::NoSuchObject(object))?;
        if o.face_ordinal(face).is_none() {
            return Err(LooksError::NoSuchFace { object, face }.into());
        }
        Ok(self.face_material(object, face).map(str::to_string))
    }

    /// Texture images by path.
    pub fn textures(&self) -> &BTreeMap<String, TextureImage> {
        &self.textures
    }

    /// The document's drawing style (`None`: the default).
    pub fn style(&self) -> Option<&Style> {
        self.style.as_ref()
    }

    /// The document's sun and shadows (`None`: the default, off).
    pub fn shadows(&self) -> Option<&Shadows> {
        self.shadows.as_ref()
    }

    /// Pixel size of a material's texture, when the model holds its image.
    pub fn texture_px(&self, m: &Material) -> Option<(u32, u32)> {
        m.texture
            .as_deref()
            .and_then(|t| self.textures.get(t))
            .map(|t| (t.width, t.height))
    }

    pub(crate) fn check_material_fields(m: &Material) -> Result<(), KernelError> {
        let size_ok = m.size.is_none_or(|s| {
            s.iter()
                .all(|x| x.is_finite() && (2.54e-5..=1000.0).contains(x))
        });
        let ok = size_ok
            && m.rotation.is_finite()
            && m.rotation.abs() <= 1e6
            && m.offset
                .iter()
                .all(|o| o.is_finite() && o.abs() <= crate::tolerance::MAX_COORD);
        if !ok {
            return Err(LooksError::BadMaterial.into());
        }
        if let Some(s) = &m.stock
            && s.species.chars().count() > 64
        {
            return Err(DocumentError::BadName.into());
        }
        Ok(())
    }

    /// [`crate::model::Op::DefineMaterial`].
    pub(crate) fn define_material(
        &mut self,
        name: &str,
        mut m: Material,
    ) -> Result<OpOutcome, KernelError> {
        if name.trim().is_empty() || name.chars().count() > 64 || name.trim() != name {
            return Err(DocumentError::BadName.into());
        }
        Self::check_material_fields(&m)?;
        m.rotation = norm_deg(m.rotation);
        if let Some(s) = &mut m.stock {
            s.species = s.species.trim().to_string();
        }
        self.materials.insert(name.to_string(), m);
        Ok(OpOutcome::MaterialSet)
    }

    /// Every material reference in the model (solids, faces, meshes, groups, instances and
    /// the definitions' templates), for renaming and deleting.
    fn each_material_ref(&mut self, f: &mut dyn FnMut(&mut Option<String>)) {
        fn solid(o: &mut Object, f: &mut dyn FnMut(&mut Option<String>)) {
            f(&mut o.material);
            for p in o.paint.values_mut() {
                f(&mut p.material);
            }
            o.paint.retain(|_, p| !p.is_default());
        }
        for o in self.objects.values_mut() {
            solid(o, f);
        }
        for m in self.meshes.values_mut() {
            f(&mut m.material);
        }
        for n in self.nodes.values_mut() {
            f(&mut n.material);
        }
        for d in self.defs.values_mut() {
            for m in d.members.values_mut() {
                f(&mut m.material);
                match &mut m.kind {
                    MemberKind::Solid(o) => solid(o, f),
                    MemberKind::Mesh(x) => f(&mut x.material),
                    _ => {}
                }
            }
        }
    }

    /// [`crate::model::Op::RenameMaterial`]: everything painted with it follows.
    pub(crate) fn rename_material(
        &mut self,
        name: &str,
        to: &str,
    ) -> Result<OpOutcome, KernelError> {
        if to.trim().is_empty() || to.chars().count() > 64 || to.trim() != to {
            return Err(DocumentError::BadName.into());
        }
        let m = self
            .materials
            .get(name)
            .cloned()
            .ok_or_else(|| DocumentError::NoSuchMaterial(name.to_string()))?;
        if name == to {
            return Ok(OpOutcome::MaterialSet);
        }
        if self.materials.contains_key(to) {
            return Err(LooksError::MaterialExists(to.to_string()).into());
        }
        // The nesting default (lane 9) is keyed by the name the cut list reads: when that is
        // the material's own name (no species linked), the default follows the new name,
        // unless the new name already has one of its own.
        let reads_by_name = m.stock.as_ref().is_none_or(|l| l.species.trim().is_empty());
        let carried = self
            .stock
            .as_ref()
            .and_then(|l| l.default_for(name))
            .filter(|_| reads_by_name)
            .map(str::to_string);
        self.materials.remove(name);
        self.materials.insert(to.to_string(), m);
        self.each_material_ref(&mut |r| {
            if r.as_deref() == Some(name) {
                *r = Some(to.to_string());
            }
        });
        if let Some(stock) = carried
            && self.stock_library().default_for(to).is_none()
        {
            self.edit_stock(|l| {
                l.set_default(name, None)?;
                l.set_default(to, Some(&stock))
            })?;
        }
        Ok(OpOutcome::MaterialSet)
    }

    /// [`crate::model::Op::DeleteMaterial`]: what was painted with it goes back to the
    /// default (or what is around it).
    pub(crate) fn delete_material(&mut self, name: &str) -> Result<OpOutcome, KernelError> {
        if self.materials.remove(name).is_none() {
            return Err(DocumentError::NoSuchMaterial(name.to_string()).into());
        }
        self.each_material_ref(&mut |r| {
            if r.as_deref() == Some(name) {
                *r = None;
            }
        });
        Ok(OpOutcome::MaterialSet)
    }

    /// [`crate::model::Op::AddTexture`]: the same bytes again under the same path change
    /// nothing; other bytes replace the image (every material naming it shows the new one).
    pub(crate) fn add_texture(
        &mut self,
        path: &str,
        mime: &str,
        data: &[u8],
    ) -> Result<OpOutcome, KernelError> {
        check_texture_path(path)?;
        let img = TextureImage::new(mime, data.to_vec())?;
        if !self.textures.contains_key(path) && self.textures.len() >= MAX_TEXTURES {
            return Err(LooksError::TooManyTextures.into());
        }
        self.textures.insert(path.to_string(), img);
        Ok(OpOutcome::TextureAdded {
            path: path.to_string(),
        })
    }

    fn check_paint_material(&self, material: Option<&str>) -> Result<(), KernelError> {
        if let Some(m) = material
            && !self.materials.contains_key(m)
        {
            return Err(DocumentError::NoSuchMaterial(m.to_string()).into());
        }
        Ok(())
    }

    fn unlocked(&self, t: Thing) -> Result<(), KernelError> {
        let locked = match t {
            Thing::Object(k) => self.objects[k].member.locked,
            Thing::Node(k) => self.nodes[k].member.locked,
            Thing::Mesh(k) => self.meshes[k].member.locked,
        };
        if locked {
            return Err(StructureError::Locked.into());
        }
        Ok(())
    }

    /// [`crate::model::Op::Paint`].
    pub(crate) fn paint(
        &mut self,
        target: &PaintTarget,
        material: Option<&str>,
    ) -> Result<OpOutcome, KernelError> {
        self.check_paint_material(material)?;
        match target {
            PaintTarget::Faces { object, faces } => {
                if faces.is_empty() {
                    return Err(LooksError::NothingToPaint.into());
                }
                let k = from_public::<ObjectId>(*object);
                if !self.objects.contains_key(k) {
                    return Err(DocumentError::NoSuchObject(*object).into());
                }
                self.unlocked(Thing::Object(k))?;
                let o = &mut self.objects[k];
                for &f in faces {
                    if o.face_ordinal(f).is_none() {
                        return Err(LooksError::NoSuchFace {
                            object: *object,
                            face: f,
                        }
                        .into());
                    }
                }
                for &f in faces {
                    let e = o.paint.entry(f).or_default();
                    e.material = material.map(str::to_string);
                    if e.is_default() {
                        o.paint.remove(&f);
                    }
                }
                let p = o.member.parent;
                self.mark(p);
            }
            PaintTarget::Items { items } => {
                if items.is_empty() {
                    return Err(LooksError::NothingToPaint.into());
                }
                let things = self.things(items)?;
                for &t in &things {
                    self.unlocked(t)?;
                }
                for t in things {
                    match t {
                        Thing::Object(k) => self.objects[k].material = material.map(str::to_string),
                        Thing::Node(k) => self.nodes[k].material = material.map(str::to_string),
                        Thing::Mesh(k) => self.meshes[k].material = material.map(str::to_string),
                    }
                    self.mark(self.parent_of(t));
                }
            }
        }
        Ok(OpOutcome::MaterialSet)
    }

    /// [`crate::model::Op::SetFaceUv`]: how the texture sits on one face (`None`: the
    /// material's own placement).
    pub(crate) fn set_face_uv(
        &mut self,
        object: u64,
        face: u64,
        uv: Option<&UvXform>,
    ) -> Result<OpOutcome, KernelError> {
        let k = from_public::<ObjectId>(object);
        if !self.objects.contains_key(k) {
            return Err(DocumentError::NoSuchObject(object).into());
        }
        self.unlocked(Thing::Object(k))?;
        let uv = match uv {
            Some(x) => x.checked()?,
            None => UvXform::default(),
        };
        let o = &mut self.objects[k];
        if o.face_ordinal(face).is_none() {
            return Err(LooksError::NoSuchFace { object, face }.into());
        }
        let e = o.paint.entry(face).or_default();
        e.uv = uv;
        if e.is_default() {
            o.paint.remove(&face);
        }
        let p = o.member.parent;
        self.mark(p);
        Ok(OpOutcome::MaterialSet)
    }

    /// [`crate::model::Op::SetStyle`].
    pub(crate) fn set_style(&mut self, style: Option<&Style>) -> Result<OpOutcome, KernelError> {
        if let Some(s) = style {
            s.check()?;
        }
        self.style = style.cloned().filter(|s| *s != Style::default());
        Ok(OpOutcome::StyleSet)
    }

    /// [`crate::model::Op::SetShadows`].
    pub(crate) fn set_shadows(&mut self, s: Option<&Shadows>) -> Result<OpOutcome, KernelError> {
        if let Some(s) = s {
            s.check()?;
        }
        self.shadows = s.copied();
        Ok(OpOutcome::StyleSet)
    }

    pub(crate) fn check_scene_looks(sc: &crate::model::SceneDef) -> Result<(), KernelError> {
        if let Some(s) = &sc.style {
            s.check()?;
        }
        if let Some(s) = &sc.shadows {
            s.check()?;
        }
        Ok(())
    }

    /// Faces painted with a material that is not defined, textures named by nothing, face
    /// paint on faces that are gone: the validator's checks for this module.
    pub(crate) fn check_looks(&self) -> Result<(), String> {
        let known =
            |m: &Option<String>| m.as_deref().is_none_or(|m| self.materials.contains_key(m));
        let solid_ok = |o: &Object| -> Result<(), String> {
            for (&f, p) in &o.paint {
                if o.face_ordinal(f).is_none() {
                    return Err("paint on a face that does not exist".into());
                }
                if p.is_default() {
                    return Err("an empty face paint entry was kept".into());
                }
                if !known(&p.material) {
                    return Err("a face's material is not defined".into());
                }
            }
            Ok(())
        };
        for o in self.objects.values() {
            solid_ok(o)?;
        }
        for n in self.nodes.values() {
            if !known(&n.material) {
                return Err("a group's or component's material is not defined".into());
            }
        }
        for d in self.defs.values() {
            for m in d.members.values() {
                if !known(&m.material) {
                    return Err("a member's material is not defined".into());
                }
                if let MemberKind::Solid(o) = &m.kind {
                    solid_ok(o)?;
                }
            }
        }
        for (name, m) in &self.materials {
            if name.trim().is_empty() {
                return Err("a material has no name".into());
            }
            if Self::check_material_fields(m).is_err() {
                return Err("a material's texture placement is out of range".into());
            }
        }
        Ok(())
    }

    /// Anything this module added, so older documents keep their hashes.
    pub(crate) fn has_looks(&self) -> bool {
        !self.textures.is_empty()
            || self.style.is_some()
            || self.shadows.is_some()
            || self.objects.values().any(|o| !o.paint.is_empty())
            || self.nodes.values().any(|n| n.material.is_some())
            || self.defs.values().any(|d| {
                d.members.values().any(|m| {
                    m.material.is_some()
                        || matches!(&m.kind, MemberKind::Solid(o) if !o.paint.is_empty())
                })
            })
            || self.materials.values().any(|m| !m.is_plain())
            || self
                .scenes
                .iter()
                .any(|s| s.style.is_some() || s.shadows.is_some())
    }

    pub(crate) fn hash_looks(&self, h: &mut StableHasher) {
        h.tag("looks");
        for (name, m) in &self.materials {
            if !m.is_plain() {
                h.tag(name);
                if let Some(s) = m.size {
                    h.len(s[0]);
                    h.len(s[1]);
                }
                h.u64(fbits(m.rotation));
                h.len(m.offset[0]);
                h.len(m.offset[1]);
                match &m.stock {
                    Some(s) => {
                        h.u64(match s.kind {
                            StockKind::Lumber => 1,
                            StockKind::Sheet => 2,
                        });
                        h.tag(&s.species);
                    }
                    None => h.u64(0),
                }
            }
        }
        h.usize(self.textures.len());
        for (path, t) in &self.textures {
            h.tag(path);
            h.tag(&t.mime);
            h.u64(t.digest);
        }
        for (id, o) in self.objects() {
            if !o.paint.is_empty() {
                h.u64(id);
                hash_paint(h, o);
            }
        }
        for (id, n) in self.nodes() {
            if let Some(m) = &n.material {
                h.u64(id);
                h.tag(m);
            }
        }
        for (id, d) in self.definitions() {
            for (&slot, m) in &d.members {
                let paint = matches!(&m.kind, MemberKind::Solid(o) if !o.paint.is_empty());
                if m.material.is_some() || paint {
                    h.u64(id);
                    h.u64(u64::from(slot));
                    h.tag(m.material.as_deref().unwrap_or(""));
                    if let MemberKind::Solid(o) = &m.kind {
                        hash_paint(h, o);
                    }
                }
            }
        }
        if let Some(s) = &self.style {
            s.hash_into(h);
        }
        if let Some(s) = &self.shadows {
            s.hash_into(h);
        }
        for sc in &self.scenes {
            if let Some(s) = &sc.style {
                h.tag(&sc.name);
                s.hash_into(h);
            }
            if let Some(s) = &sc.shadows {
                h.tag(&sc.name);
                s.hash_into(h);
            }
        }
    }

    /// The material covering the most area of these solids (face paint, else the solid's,
    /// else what is around it); ties go to the name that sorts first. What the cut list
    /// calls a part's material.
    pub(crate) fn majority_material(
        &self,
        solids: &[&Object],
        around: Option<&str>,
    ) -> Option<String> {
        let mut area: BTreeMap<Option<&str>, f64> = BTreeMap::new();
        for o in solids {
            // A template solid has no parents: `around` is its instance's paint.
            let default = self.solid_default(o).or(around);
            let b = o.brep();
            for (_, f) in b.faces() {
                let m = o
                    .paint
                    .get(&f.id)
                    .and_then(|p| p.material.as_deref())
                    .or(default);
                let a: f64 = f
                    .loops()
                    .map(|l| {
                        let pts: Vec<DVec2> = b
                            .loop_points(l)
                            .iter()
                            .map(|&p| f.plane.to_local(p))
                            .collect();
                        sawhorse_tessellate::signed_area(&pts)
                    })
                    .sum();
                *area.entry(m).or_default() += a.abs();
            }
        }
        // Largest area; on a tie the material that sorts first (None sorts first of all, so
        // a half-painted part only takes the paint when the paint covers more).
        let mut best: Option<(Option<&str>, f64)> = None;
        for (m, a) in area {
            if best.is_none_or(|(_, b)| a > b * (1.0 + 1e-9)) {
                best = Some((m, a));
            }
        }
        best.and_then(|(m, _)| m.map(str::to_string))
    }

    /// The cut list's reading of a material name: the stock link's species (or the name) and
    /// whether the link says sheet goods.
    pub(crate) fn stock_reading(&self, material: Option<String>) -> (Option<String>, Option<bool>) {
        let Some(name) = material else {
            return (None, None);
        };
        match self.materials.get(&name).and_then(|m| m.stock.as_ref()) {
            Some(link) => {
                let text = if link.species.trim().is_empty() {
                    name
                } else {
                    link.species.clone()
                };
                (Some(text), Some(link.kind == StockKind::Sheet))
            }
            None => (Some(name), None),
        }
    }

    /// A group's or instance's own material (its default).
    pub fn node_material(&self, node: u64) -> Option<&str> {
        let k = self.node_key(node).ok()?;
        self.nodes[k].material.as_deref()
    }
}

fn hash_paint(h: &mut StableHasher, o: &Object) {
    h.tag("paint");
    h.usize(o.paint.len());
    for (&f, p) in &o.paint {
        h.u64(f);
        h.tag(p.material.as_deref().unwrap_or(""));
        if !p.uv.is_identity() {
            h.u64(fbits(p.uv.scale[0]));
            h.u64(fbits(p.uv.scale[1]));
            h.u64(fbits(p.uv.rotation));
            h.len(p.uv.offset[0]);
            h.len(p.uv.offset[1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for n in 0..40usize {
            let d: Vec<u8> = (0..n).map(|i| (i * 37 + n) as u8).collect();
            assert_eq!(b64::decode(&b64::encode(&d)).unwrap(), d);
        }
        assert_eq!(b64::encode(b"Man"), "TWFu");
        assert_eq!(b64::encode(b"Ma"), "TWE=");
        assert_eq!(b64::encode(b"M"), "TQ==");
        assert!(b64::decode("TQ=").is_none());
        assert!(b64::decode("T=Q=").is_none());
        assert!(b64::decode("TQ==TQ==").is_none());
    }

    #[test]
    fn image_sizes_from_headers() {
        let png = crate::testing::tiny_png(3, 2, [200, 150, 90]);
        assert_eq!(image_size(&png), Some(("image/png", (3, 2))));
        // A minimal JPEG header: SOI, an APP0 segment, SOF0 with 8 bits, 5 rows, 7 columns.
        let jpg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00,
            0x05, 0x00, 0x07, 0x01, 0x01, 0x11, 0x00,
        ];
        assert_eq!(image_size(&jpg), Some(("image/jpeg", (7, 5))));
        assert_eq!(image_size(b"GIF89a.........."), None);
        assert!(TextureImage::new("image/jpeg", png.clone()).is_err());
        assert!(TextureImage::new("image/png", png).is_ok());
    }

    #[test]
    fn texture_paths() {
        assert!(check_texture_path("assets/white-oak_2.png").is_ok());
        assert!(check_texture_path("assets/a.jpg").is_ok());
        for bad in [
            "white-oak.png",
            "assets/../x.png",
            "assets/.png",
            "assets/a b.png",
            "assets/a.gif",
            "assets/",
        ] {
            assert!(check_texture_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_long_side_carries_the_grain_and_positions_compose() {
        // A 0.1 x 0.4 face in the ground plane: the long side runs along y, so the frame
        // turns a quarter.
        let outer = [
            DVec3::ZERO,
            DVec3::new(0.1, 0.0, 0.0),
            DVec3::new(0.1, 0.4, 0.0),
            DVec3::new(0.0, 0.4, 0.0),
        ];
        let frame = texture_frame(&Plane::GROUND, &outer);
        assert_eq!(frame.1, DVec3::Y);
        let m = Material {
            color: [255; 4],
            texture: None,
            size: Some([0.2, 0.1]),
            rotation: 0.0,
            offset: [0.0, 0.0],
            stock: None,
        };
        let uv = texture_uv(
            &frame,
            DVec3::new(0.0, 0.4, 0.0),
            &m,
            None,
            &UvXform::default(),
        );
        assert!((uv[0] - 2.0).abs() < 1e-12 && uv[1].abs() < 1e-12, "{uv:?}");
        // A quarter turn of the texture: the point along the grain now reads along -y.
        let turned = UvXform {
            rotation: 90.0,
            ..Default::default()
        };
        let uv = texture_uv(&frame, DVec3::new(0.0, 0.4, 0.0), &m, None, &turned);
        assert!(uv[0].abs() < 1e-9 && (uv[1] + 4.0).abs() < 1e-9, "{uv:?}");
        // Fit recovers a positioning from its own affine map.
        let want = UvXform {
            scale: [1.5, 0.75],
            rotation: 30.0,
            offset: [0.02, -0.01],
        };
        let at = |p: DVec3| texture_uv(&frame, p, &m, None, &want);
        let o = at(DVec3::ZERO);
        let ux = at(DVec3::X);
        let uy = at(DVec3::Y);
        let uz = at(DVec3::Z);
        let a = [ux[0] - o[0], uy[0] - o[0], uz[0] - o[0], o[0]];
        let b = [ux[1] - o[1], uy[1] - o[1], uz[1] - o[1], o[1]];
        let got = fit_uv(&frame, &m, None, a, b).unwrap();
        assert!((got.scale[0] - 1.5).abs() < 1e-9 && (got.scale[1] - 0.75).abs() < 1e-9);
        assert!((got.rotation - 30.0).abs() < 1e-9, "{got:?}");
        assert!((got.offset[0] - 0.02).abs() < 1e-9 && (got.offset[1] + 0.01).abs() < 1e-9);
    }

    #[test]
    fn the_sun_rises_in_the_east_and_peaks_at_noon() {
        let mut s = Shadows {
            month: 3,
            day: 21,
            latitude: 40.0,
            ..Default::default()
        };
        s.minutes = 12 * 60;
        let noon = s.sun_direction();
        // Equinox noon at 40 N: altitude about 50 degrees, due south.
        assert!((noon[2].asin().to_degrees() - 50.0).abs() < 1.5, "{noon:?}");
        assert!(noon[1] < 0.0 && noon[0].abs() < 1e-9);
        s.minutes = 7 * 60;
        let morning = s.sun_direction();
        assert!(morning[0] > 0.5, "morning sun in the east: {morning:?}");
        s.minutes = 15 * 60;
        let afternoon = s.sun_direction();
        assert!(afternoon[0] < 0.0 && afternoon[2] > 0.3, "{afternoon:?}");
        s.minutes = 0;
        assert!(s.sun_direction()[2] < 0.0, "midnight: below the horizon");
        assert!(
            Shadows {
                day: 30,
                month: 2,
                ..Default::default()
            }
            .check()
            .is_err()
        );
        assert!(Shadows::default().check().is_ok());
    }
}
